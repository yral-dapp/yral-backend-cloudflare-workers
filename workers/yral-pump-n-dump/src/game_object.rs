use std::collections::{hash_map, HashMap};

use candid::{Nat, Principal};
use futures::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::{
    backend_impl::{GameBackend, GameBackendImpl},
    consts::{GDOLLR_TO_E8S, TIDE_SHIFT_DELTA},
    user_reconciler::{AddRewardReq, CompletedGameInfo, DecrementReq, StateDiff},
    websocket::GameDirection,
};

#[durable_object]
pub struct GameState {
    state: State,
    env: Env,
    start_epoch_ms: Option<u64>,
    pumps: Option<u64>,
    dumps: Option<u64>,
    // Principal: (pumps, dumps)
    bets: Option<HashMap<Principal, [u64; 2]>>,
    backend: GameBackend,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct GameObjReq {
    pub sender: Principal,
    pub direction: GameDirection,
    pub creator: Principal,
    pub token_root: Principal,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum GameResult {
    Winner,
    Looser,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct BetsResponse {
    pub pumps: u64,
    pub dumps: u64,
}

struct RewardIter {
    pub liquidity_pool: Nat,
    token_root: Principal,
    reward_pool: Nat,
    remaining: Nat,
    creator: Option<Principal>,
    outcome: GameDirection,
    bet_cnt: u64,
    inner: hash_map::IntoIter<Principal, [u64; 2]>,
}

impl Iterator for RewardIter {
    type Item = (Principal, StateDiff);

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
                StateDiff::CreatorReward(self.liquidity_pool.clone() + self.remaining.clone()),
            ));
        };
        // (bet_cnt_for_user / total_bets) * reward_pool
        // basically the user's reward proprtional to the number of their correct bets
        let bet_idx = if matches!(self.outcome, GameDirection::Pump) {
            0
        } else {
            1
        };
        let reward = (bet[bet_idx] * self.reward_pool.clone()) / self.bet_cnt;
        assert!(self.remaining >= reward);
        self.remaining -= reward.clone();

        Some((
            better,
            StateDiff::CompletedGame(CompletedGameInfo {
                pumps: bet[0],
                dumps: bet[1],
                reward,
                token_root: self.token_root,
                outcome: self.outcome,
            }),
        ))
    }
}

impl RewardIter {
    pub fn new(
        pumps: u64,
        dumps: u64,
        creator: Principal,
        token_root: Principal,
        bets: HashMap<Principal, [u64; 2]>,
    ) -> Self {
        let total = Nat::from(GDOLLR_TO_E8S) * (pumps + dumps);

        // 5% of total
        // divisible by 100, as GDOLLR_TO_E8S is also divisible by 100
        let creator_reward = (total.clone() * 5u32) / 100u32;
        // 5% of total
        let liquidity_pool = creator_reward.clone();

        let remaining = total - creator_reward.clone() - liquidity_pool.clone();
        let (outcome, bet_cnt) = if pumps > dumps {
            (GameDirection::Pump, pumps)
        } else {
            (GameDirection::Dump, dumps)
        };

        Self {
            liquidity_pool,
            reward_pool: remaining.clone(),
            remaining,
            creator: Some(creator),
            token_root,
            outcome,
            bet_cnt,
            inner: bets.into_iter(),
        }
    }
}

impl GameState {
    async fn pumps(&mut self) -> &mut u64 {
        if self.pumps.is_some() {
            return self.pumps.as_mut().unwrap();
        }

        let pumps = self.state.storage().get("pumps").await.unwrap_or_default();
        self.pumps = Some(pumps);
        self.pumps.as_mut().unwrap()
    }

    async fn dumps(&mut self) -> &mut u64 {
        if self.dumps.is_some() {
            return self.dumps.as_mut().unwrap();
        }

        let dumps = self.state.storage().get("dumps").await.unwrap_or_default();
        self.dumps = Some(dumps);
        self.dumps.as_mut().unwrap()
    }

    async fn start_epoch_ms(&mut self) -> &mut u64 {
        if self.start_epoch_ms.is_some() {
            return self.start_epoch_ms.as_mut().unwrap();
        }

        let start_epoch_ms = self
            .state
            .storage()
            .get("start_epoch_ms")
            .await
            .unwrap_or_default();
        self.start_epoch_ms = Some(start_epoch_ms);
        self.start_epoch_ms.as_mut().unwrap()
    }

    async fn bets(&mut self) -> &mut HashMap<Principal, [u64; 2]> {
        if self.bets.is_some() {
            return self.bets.as_mut().unwrap();
        }

        let bets_index = self
            .state
            .storage()
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

        self.bets = Some(bets);
        self.bets.as_mut().unwrap()
    }

    fn user_state_stub(&self, user: Principal) -> Result<Stub> {
        let user_state = self.env.durable_object("USER_EPHEMERAL_STATE")?;
        let user_state_obj = user_state.id_from_name(&user.to_string())?;

        user_state_obj.get_stub()
    }

    async fn send_reward_to_user(&self, user: Principal, state_diff: StateDiff) -> Result<()> {
        let body = AddRewardReq {
            state_diff,
            user_canister: user,
        };
        let mut req_init = RequestInit::new();
        let req = Request::new_with_init(
            "http://fake_url.com/add_reward",
            req_init
                .with_method(Method::Post)
                .with_body(Some(serde_wasm_bindgen::to_value(&body)?)),
        )?;

        let user_state = self.user_state_stub(user)?;
        user_state.fetch_with_request(req).await?;

        Ok(())
    }

    async fn round_end(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
    ) -> Result<GameResult> {
        let start_epoch_ms = Date::now().as_millis() + 10 * 10000;

        let rewards = RewardIter::new(
            *self.pumps().await,
            *self.dumps().await,
            game_creator,
            token_root,
            std::mem::take(self.bets().await),
        );
        self.state
            .storage()
            .transaction(move |mut txn| async move {
                txn.delete_all().await?;
                txn.put("start_epoch_ms", start_epoch_ms).await?;

                Ok(())
            })
            .await?;
        self.pumps = Some(0);
        self.dumps = Some(0);
        self.start_epoch_ms = Some(start_epoch_ms);

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
        let bets = self.bets().await.entry(sender).or_insert([0, 0]);
        bets[0] += 1;
        let bets = *bets;

        let dumps = *self.dumps().await;
        let pumps = self.pumps().await;
        let tide_shifted = Self::tide_shift_check(*pumps, dumps);
        *pumps += 1;

        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.state
            .storage()
            .put(&format!("bets-{sender}"), bets)
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
        let bets = self.bets().await.entry(sender).or_insert([0, 0]);
        bets[1] += 1;
        let bets = *bets;

        let pumps = *self.pumps().await;
        let dumps = self.dumps().await;
        let tide_shifted = Self::tide_shift_check(*dumps, pumps);
        *dumps += 1;

        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.state
            .storage()
            .put(&format!("bets-{sender}"), bets)
            .await?;
        self.state.storage().put("dumps", self.dumps).await?;

        Ok(GameResult::Looser)
    }

    async fn game_request(&mut self, game_req: GameObjReq) -> Result<Response> {
        if *self.start_epoch_ms().await > Date::now().as_millis() {
            return Response::error("game has not started", 503);
        }

        let user_state = self.user_state_stub(game_req.sender)?;
        let mut req_init = RequestInit::new();
        let body = DecrementReq {
            user_canister: game_req.sender,
            token_root: game_req.token_root,
        };
        let req = Request::new_with_init(
            "http://fake_url.com/decrement",
            req_init
                .with_method(Method::Post)
                .with_body(Some(serde_wasm_bindgen::to_value(&body)?)),
        )?;

        let res = user_state.fetch_with_request(req).await?;
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

#[durable_object]
impl DurableObject for GameState {
    fn new(state: State, env: Env) -> Self {
        let backend = GameBackend::new(&env).unwrap();

        Self {
            state,
            env,
            start_epoch_ms: None,
            pumps: None,
            dumps: None,
            bets: None,
            backend,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);
        router
            .get_async("/bets/:user_canister", |_req, ctx| async move {
                let user_canister_raw = ctx.param("user_canister").unwrap();
                let Ok(user_canister) = Principal::from_text(user_canister_raw) else {
                    return Response::error("Invalid user_canister", 400);
                };

                let this = ctx.data;
                let bets = this
                    .bets()
                    .await
                    .get(&user_canister)
                    .copied()
                    .unwrap_or_default();

                Response::from_json(&BetsResponse {
                    pumps: bets[0],
                    dumps: bets[1],
                })
            })
            .post_async("/bet", |mut req, ctx| async move {
                let game_req: GameObjReq = req.json().await?;
                let this = ctx.data;

                this.game_request(game_req).await
            })
            .get_async("/status", |_req, ctx| async move {
                let this = ctx.data;
                if *this.start_epoch_ms().await > Date::now().as_millis() {
                    Response::error("game has not started", 503)
                } else {
                    Response::ok("ready")
                }
            })
            .run(req, env)
            .await
    }
}
