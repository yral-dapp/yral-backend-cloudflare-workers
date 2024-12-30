use std::{cell::RefCell, rc::Rc};

use candid::{Nat, Principal};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::{
    backend_impl::{GameBackend, GameBackendImpl},
    balance_object::{AddRewardReq, DecrementReq},
    consts::GDOLLR_TO_E8S,
    websocket::GameDirection,
};

#[durable_object]
pub struct GameState {
    state: State,
    env: Env,
    start_epoch_ms: u64,
    pumps: u64,
    dumps: u64,
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

struct Reward {
    creator: Nat,
    liquidity_pool: Nat,
    winner: Nat,
}

impl Reward {
    pub fn from_pumps_and_dumps(pumps: u64, dumps: u64) -> Self {
        let total = Nat::from(GDOLLR_TO_E8S) * (pumps + dumps);

        // 5% of total
        // divisible by 100, as GDOLLR_TO_E8S is also divisible by 100
        let creator = (total.clone() * 5u32) / 100u32;
        // 5% of total
        let liquidity_pool = creator.clone();

        let winner = total - creator.clone() - liquidity_pool.clone();

        Self {
            creator,
            liquidity_pool,
            winner,
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
        winner: Principal,
    ) -> Result<GameResult> {
        let reward = Reward::from_pumps_and_dumps(self.pumps, self.dumps);

        let mut storage = self.state.storage();
        let start_epoch_ms = Date::now().as_millis() + 10 * 10000;
        storage
            .transaction(move |mut txn| async move {
                txn.put("pumps", 0u64).await?;
                txn.put("dumps", 0u64).await?;
                txn.put("start_epoch_ms", start_epoch_ms).await?;

                Ok(())
            })
            .await?;

        self.pumps = 0;
        self.dumps = 0;
        self.start_epoch_ms = start_epoch_ms;

        self.send_reward_to_user(winner, reward.winner).await?;
        self.send_reward_to_user(game_creator, reward.creator)
            .await?;
        self.backend
            .add_dollr_to_liquidity_pool(game_creator, token_root, reward.liquidity_pool)
            .await?;

        Ok(GameResult::Winner)
    }

    async fn increment_pumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<GameResult> {
        let prev_pumps = self.pumps;
        self.pumps += 1;
        if self.pumps > self.dumps && prev_pumps < self.dumps {
            return self.round_end(game_creator, token_root, sender).await;
        }

        self.state.storage().put("pumps", self.pumps).await?;

        Ok(GameResult::Looser)
    }

    async fn increment_dumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<GameResult> {
        let prev_dumps = self.dumps;
        self.dumps += 1;
        if self.dumps > self.pumps && prev_dumps < self.pumps {
            return self.round_end(game_creator, token_root, sender).await;
        }

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
}

impl InitState {
    async fn initialize(storage: Storage) -> Self {
        let start_epoch_ms = storage.get("start_time_ms").await.unwrap_or(0);
        let pumps = storage.get("pumps").await.unwrap_or(0);
        let dumps = storage.get("dumps").await.unwrap_or(0);

        Self {
            start_epoch_ms,
            pumps,
            dumps,
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
