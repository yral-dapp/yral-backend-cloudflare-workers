use std::{
    cell::RefCell,
    collections::{hash_map, HashMap},
    rc::Rc,
};

use candid::{Nat, Principal};
use futures::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::{
    backend_impl::{GameBackend, GameBackendImpl},
    balance_object::{AddRewardReq, DecrementReq},
    consts::{GDOLLR_TO_E8S, TIDE_SHIFT_DELTA},
    websocket::GameDirection,
};

#[durable_object]
pub struct GameState {
    state: State,
    env: Env,
    start_epoch_ms: u64,
    pumps: u64,
    dumps: u64,
    // Principal: (pumps, dumps)
    bets: HashMap<Principal, [u64; 2]>,
    backend: GameBackend,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct GameObjReq {
    pub sender: Principal,
    pub direction: GameDirection,
    pub creator: Principal,
    pub token_root: Principal,
}

#[derive(Serialize, Deserialize)]
pub enum GameResult {
    Winner,
    Looser,
}

struct RewardIter {
    pub liquidity_pool: Nat,
    reward_pool: Nat,
    remaining: Nat,
    creator: Option<Principal>,
    bet_idx: usize,
    bet_cnt: u64,
    inner: hash_map::IntoIter<Principal, [u64; 2]>,
}

impl Iterator for RewardIter {
    type Item = (Principal, Nat);

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (start, end) = self.inner.size_hint();
        let extra = self.creator.as_ref().map(|_| 1).unwrap_or_default();

        (start + extra, end.map(|e| e + extra))
    }

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.inner.next();
        let Some((better, bet)) = next else {
            let creator = self.creator.take()?;
            // there may be something remaining due to rounding errors
            return Some((
                creator,
                self.liquidity_pool.clone() + self.remaining.clone(),
            ));
        };
        // (bet_cnt_for_user / total_bets) * reward_pool
        // basically the user's reward proprtional to the number of their correct bets
        let reward = (bet[self.bet_idx] * self.reward_pool.clone()) / self.bet_cnt;
        assert!(self.remaining >= reward);
        self.remaining -= reward.clone();

        Some((better, reward))
    }
}

impl RewardIter {
    pub fn new(
        pumps: u64,
        dumps: u64,
        creator: Principal,
        bets: HashMap<Principal, [u64; 2]>,
    ) -> Self {
        let total = Nat::from(GDOLLR_TO_E8S) * (pumps + dumps);

        // 5% of total
        // divisible by 100, as GDOLLR_TO_E8S is also divisible by 100
        let creator_reward = (total.clone() * 5u32) / 100u32;
        // 5% of total
        let liquidity_pool = creator_reward.clone();

        let remaining = total - creator_reward.clone() - liquidity_pool.clone();
        let (bet_idx, bet_cnt) = if pumps > dumps {
            (0, pumps)
        } else {
            (1, dumps)
        };

        Self {
            liquidity_pool,
            reward_pool: remaining.clone(),
            remaining,
            creator: Some(creator),
            bet_idx,
            bet_cnt,
            inner: bets.into_iter(),
        }
    }
}

impl GameState {
    fn user_balance_stub(&self, user: Principal) -> Result<Stub> {
        let user_dollr_balance = self.env.durable_object("USER_DOLLR_BALANCE")?;
        let user_bal_obj = user_dollr_balance.id_from_name(&user.to_string())?;

        user_bal_obj.get_stub()
    }

    async fn send_reward_to_user(&self, user: Principal, amount: Nat) -> Result<()> {
        let body = AddRewardReq {
            amount,
            user_canister: user,
        };
        let mut req_init = RequestInit::new();
        let req = Request::new_with_init(
            "http://fake_url.com/add_reward",
            req_init
                .with_method(Method::Post)
                .with_body(Some(serde_wasm_bindgen::to_value(&body)?)),
        )?;

        let user_bal_stub = self.user_balance_stub(user)?;
        user_bal_stub.fetch_with_request(req).await?;

        Ok(())
    }

    async fn round_end(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
    ) -> Result<GameResult> {
        let start_epoch_ms = Date::now().as_millis() + 10 * 10000;
        self.state.storage().delete_all().await?;

        let rewards = RewardIter::new(
            self.pumps,
            self.dumps,
            game_creator,
            std::mem::take(&mut self.bets),
        );
        self.pumps = 0;
        self.dumps = 0;
        self.start_epoch_ms = start_epoch_ms;

        let lp_reward = rewards.liquidity_pool.clone();
        let mut reward_futs = rewards
            .map(|(winner, reward)| self.send_reward_to_user(winner, reward))
            .collect::<FuturesUnordered<_>>();

        while reward_futs.next().await.is_some() {}
        std::mem::drop(reward_futs);

        self.backend
            .add_dollr_to_liquidity_pool(game_creator, token_root, lp_reward)
            .await?;

        Ok(GameResult::Winner)
    }

    fn tide_shift_check(with: u64, other: u64) -> bool {
        let prev_delta = with.saturating_sub(other);
        let new_delta = (with + 1).saturating_sub(other);

        prev_delta < TIDE_SHIFT_DELTA && new_delta >= TIDE_SHIFT_DELTA
    }

    async fn increment_pumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<GameResult> {
        let bets = self.bets.entry(sender).or_insert([0, 0]);
        bets[0] += 1;

        let tide_shifted = Self::tide_shift_check(self.pumps, self.dumps);
        self.pumps += 1;
        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.state
            .storage()
            .put(&format!("bets-{sender}"), *bets)
            .await?;
        self.state.storage().put("pumps", self.pumps).await?;

        Ok(GameResult::Looser)
    }

    async fn increment_dumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<GameResult> {
        let bets = self.bets.entry(sender).or_insert([0, 0]);
        bets[1] += 1;

        let tide_shifted = Self::tide_shift_check(self.dumps, self.pumps);
        self.dumps += 1;
        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.state
            .storage()
            .put(&format!("bets-{sender}"), *bets)
            .await?;
        self.state.storage().put("dumps", self.dumps).await?;

        Ok(GameResult::Looser)
    }

    async fn game_request(&mut self, game_req: GameObjReq) -> Result<Response> {
        if self.start_epoch_ms > Date::now().as_millis() {
            return Response::error("game has not started", 503);
        }

        let user_bal_stub = self.user_balance_stub(game_req.sender)?;
        let mut req_init = RequestInit::new();
        let body = DecrementReq {
            user_canister: game_req.sender,
        };
        let req = Request::new_with_init(
            "http://fake_url.com/decrement",
            req_init
                .with_method(Method::Post)
                .with_body(Some(serde_wasm_bindgen::to_value(&body)?)),
        )?;

        let res = user_bal_stub.fetch_with_request(req).await?;
        if res.status_code() != 200 {
            return Ok(res);
        }

        let game_res = match game_req.direction {
            GameDirection::Pump => {
                self.increment_pumps(game_req.creator, game_req.token_root, game_req.sender)
                    .await?
            }
            GameDirection::Dump => {
                self.increment_dumps(game_req.creator, game_req.token_root, game_req.sender)
                    .await?
            }
        };

        Response::from_json(&game_res)
    }
}

struct InitState {
    start_epoch_ms: u64,
    pumps: u64,
    dumps: u64,
    bets: HashMap<Principal, [u64; 2]>,
}

impl InitState {
    async fn initialize(storage: Storage) -> Self {
        let start_epoch_ms = storage.get("start_time_ms").await.unwrap_or_default();
        let pumps = storage.get("pumps").await.unwrap_or_default();
        let dumps = storage.get("dumps").await.unwrap_or_default();

        let bets_index = storage
            .list_with_options(ListOptions::new().prefix("bets-"))
            .await
            .unwrap_or_default();

        let mut bets = HashMap::new();
        for entry in bets_index.entries() {
            let raw_entry = entry.expect("invalid bets stored?!");
            let (key, bet): (String, [u64; 2]) =
                serde_wasm_bindgen::from_value(raw_entry).expect("invalid bets stored?!");
            let better = Principal::from_text(key.strip_prefix("bets-").unwrap()).unwrap();
            bets.insert(better, bet);
        }

        Self {
            start_epoch_ms,
            pumps,
            dumps,
            bets,
        }
    }
}

#[durable_object]
impl DurableObject for GameState {
    fn new(state: State, env: Env) -> Self {
        // all this is needed because "new" is not async
        // and state.wait_until can't have a return value :|
        let storage = state.storage();
        let init_state = Rc::new(RefCell::new(None::<InitState>));
        let init_state_ref = init_state.clone();
        state.wait_until(async move {
            let init = InitState::initialize(storage).await;
            *init_state_ref.borrow_mut() = Some(init);
        });

        let init_state = Rc::into_inner(init_state).unwrap().into_inner().unwrap();

        let backend = GameBackend::new(&env).unwrap();

        Self {
            state,
            env,
            start_epoch_ms: init_state.start_epoch_ms,
            pumps: init_state.pumps,
            dumps: init_state.dumps,
            bets: init_state.bets,
            backend,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);
        router
            .post_async("/bet", |mut req, ctx| async move {
                let game_req: GameObjReq = req.json().await?;
                let this = ctx.data;

                this.game_request(game_req).await
            })
            .get("/status", |_req, ctx| {
                let this = ctx.data;
                if this.start_epoch_ms > Date::now().as_millis() {
                    Response::error("game has not started", 503)
                } else {
                    Response::ok("ready")
                }
            })
            .run(req, env)
            .await
    }
}
