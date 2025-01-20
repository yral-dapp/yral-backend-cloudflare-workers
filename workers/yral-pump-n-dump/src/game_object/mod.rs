mod ws;

use std::collections::{hash_map, HashMap};

use crate::{
    backend_impl::{GameBackend, GameBackendImpl},
    consts::{GDOLLR_TO_E8S, TIDE_SHIFT_DELTA},
    user_reconciler::{AddRewardReq, CompletedGameInfo, DecrementReq, StateDiff},
    utils::RequestInitBuilder,
};
use candid::{Nat, Principal};
use futures::{stream::FuturesUnordered, StreamExt};
use pump_n_dump_common::{rest::UserBetsResponse, ws::GameResult, GameDirection};
use worker::*;

#[durable_object]
pub struct GameState {
    state: State,
    env: Env,
    has_tide_shifted: Option<bool>,
    round_pumps: Option<u64>,
    round_dumps: Option<u64>,
    cumulative_pumps: Option<u64>,
    cumulative_dumps: Option<u64>,
    // Principal: (pumps, dumps)
    bets: Option<HashMap<Principal, [u64; 2]>>,
    backend: GameBackend,
}

struct GameObjReq {
    pub sender: Principal,
    pub direction: GameDirection,
    pub creator: Principal,
    pub token_root: Principal,
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
    async fn pumps(&mut self) -> u64 {
        if let Some(p) = self.round_pumps {
            return p;
        }

        let pumps = self.state.storage().get("pumps").await.unwrap_or_default();
        self.round_pumps = Some(pumps);
        pumps
    }

    async fn dumps(&mut self) -> u64 {
        if let Some(d) = self.round_dumps {
            return d;
        }

        let dumps = self.state.storage().get("dumps").await.unwrap_or_default();
        self.round_dumps = Some(dumps);
        dumps
    }

    async fn cumulative_pumps(&mut self) -> u64 {
        if let Some(tot) = self.cumulative_pumps {
            return tot;
        }

        let pumps = self
            .state
            .storage()
            .get("total-pumps")
            .await
            .unwrap_or_default();
        self.cumulative_pumps = Some(pumps);
        pumps
    }

    async fn cumulative_dumps(&mut self) -> u64 {
        if let Some(tot) = self.cumulative_dumps {
            return tot;
        }

        let dumps = self
            .state
            .storage()
            .get("total-dumps")
            .await
            .unwrap_or_default();
        self.cumulative_dumps = Some(dumps);
        dumps
    }

    async fn increment_pumps_inner(&mut self) -> Result<u64> {
        let total_pumps = self.cumulative_pumps().await + 1;
        let pumps = self.pumps().await + 1;

        self.state
            .storage()
            .transaction(move |mut txn| async move {
                txn.put("total-pumps", total_pumps).await?;
                txn.put("pumps", pumps).await?;

                Ok(())
            })
            .await?;
        self.cumulative_pumps = Some(total_pumps);
        self.round_pumps = Some(pumps);

        Ok(pumps)
    }

    async fn increment_dumps_inner(&mut self) -> Result<u64> {
        let total_dumps = self.cumulative_dumps().await + 1;
        let dumps = self.dumps().await + 1;

        self.state
            .storage()
            .transaction(move |mut txn| async move {
                txn.put("total-dumps", total_dumps).await?;
                txn.put("dumps", dumps).await?;

                Ok(())
            })
            .await?;

        self.cumulative_dumps = Some(total_dumps);
        self.round_dumps = Some(dumps);

        Ok(dumps)
    }

    async fn has_tide_shifted(&mut self) -> bool {
        if let Some(shifted) = self.has_tide_shifted {
            return shifted;
        };

        let shifted = self
            .state
            .storage()
            .get("has_tide_shifted")
            .await
            .unwrap_or_default();
        self.has_tide_shifted = Some(shifted);

        shifted
    }

    async fn set_tide_shifted(&mut self) -> Result<()> {
        self.has_tide_shifted = Some(true);
        self.state.storage().put("has_tide_shifted", true).await?;

        Ok(())
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
        let req = Request::new_with_init(
            "http://fake_url.com/add_reward",
            RequestInitBuilder::default()
                .method(Method::Post)
                .json(&body)?
                .build(),
        )?;

        let user_state = self.user_state_stub(user)?;
        user_state.fetch_with_request(req).await?;

        Ok(())
    }

    async fn round_end(&mut self, game_creator: Principal, token_root: Principal) -> Result<()> {
        let total_dumps = self.cumulative_dumps().await;
        let total_pumps = self.cumulative_pumps().await;

        let rewards = RewardIter::new(
            self.pumps().await,
            self.dumps().await,
            game_creator,
            token_root,
            std::mem::take(self.bets().await),
        );

        // cleanup
        self.state.storage().delete_all().await?;
        self.round_pumps = Some(0);
        self.round_dumps = Some(0);

        self.state.storage().put("total-dumps", total_dumps).await?;
        self.state.storage().put("total-pumps", total_pumps).await?;

        let game_res = GameResult {
            direction: rewards.outcome,
            reward_pool: rewards.reward_pool.clone(),
            bet_count: rewards.bet_cnt,
        };

        let lp_reward = rewards.liquidity_pool.clone();
        let mut reward_futs = rewards
            .map(|(winner, reward)| self.send_reward_to_user(winner, reward))
            .collect::<FuturesUnordered<_>>();

        while reward_futs.next().await.is_some() {}
        std::mem::drop(reward_futs);

        self.backend
            .add_dollr_to_liquidity_pool(game_creator, token_root, lp_reward)
            .await?;

        self.broadcast_game_result(game_res)?;

        Ok(())
    }

    async fn tide_shift_check(&mut self, with: u64, other: u64) -> Result<bool> {
        let prev_delta = (with - 1).saturating_sub(other);
        let new_delta = (with).saturating_sub(other);

        let shifted = prev_delta < TIDE_SHIFT_DELTA && new_delta >= TIDE_SHIFT_DELTA;
        if !shifted {
            return Ok(false);
        }

        if !self.has_tide_shifted().await {
            self.set_tide_shifted().await?;
            return Ok(false);
        }

        Ok(true)
    }

    async fn increment_pumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<()> {
        let bets = self.bets().await.entry(sender).or_insert([0, 0]);
        bets[0] += 1;
        let bets = *bets;

        self.increment_pumps_inner().await?;

        let total_pumps = self.cumulative_pumps().await;
        let total_dumps = self.cumulative_dumps().await;
        let tide_shifted = self.tide_shift_check(total_pumps, total_dumps).await?;

        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.state
            .storage()
            .put(&format!("bets-{sender}"), bets)
            .await?;

        let pool = self.pumps().await + self.dumps().await;
        self.broadcast_pool_update(pool)?;

        Ok(())
    }

    async fn increment_dumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<()> {
        let bets = self.bets().await.entry(sender).or_insert([0, 0]);
        bets[1] += 1;
        let bets = *bets;

        self.increment_dumps_inner().await?;

        let total_pumps = self.cumulative_pumps().await;
        let total_dumps = self.cumulative_dumps().await;
        let tide_shifted = self.tide_shift_check(total_dumps, total_pumps).await?;

        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.state
            .storage()
            .put(&format!("bets-{sender}"), bets)
            .await?;

        let pool = self.pumps().await + self.dumps().await;
        self.broadcast_pool_update(pool)?;

        Ok(())
    }

    async fn game_request(&mut self, game_req: GameObjReq) -> Result<()> {
        let user_state = self.user_state_stub(game_req.sender)?;
        let body = DecrementReq {
            user_canister: game_req.sender,
            token_root: game_req.token_root,
        };
        let req = Request::new_with_init(
            "http://fake_url.com/decrement",
            RequestInitBuilder::default()
                .method(Method::Post)
                .json(&body)?
                .build(),
        )?;

        let res = user_state.fetch_with_request(req).await?;
        if res.status_code() != 200 {
            return Err(worker::Error::RustError(
                "failed to handle decrement".into(),
            ));
        }

        match game_req.direction {
            GameDirection::Pump => {
                self.increment_pumps(game_req.creator, game_req.token_root, game_req.sender)
                    .await?
            }
            GameDirection::Dump => {
                self.increment_dumps(game_req.creator, game_req.token_root, game_req.sender)
                    .await?
            }
        };

        Ok(())
    }
}

#[durable_object]
impl DurableObject for GameState {
    fn new(state: State, env: Env) -> Self {
        let backend = GameBackend::new(&env).unwrap();

        Self {
            state,
            env,
            round_pumps: None,
            round_dumps: None,
            bets: None,
            backend,
            has_tide_shifted: None,
            cumulative_pumps: None,
            cumulative_dumps: None,
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

                Response::from_json(&UserBetsResponse {
                    pumps: bets[0],
                    dumps: bets[1],
                })
            })
            .get_async(
                "/ws/:game_canister/:token_root/:user_canister",
                |req, ctx| async move {
                    let upgrade = req.headers().get("Upgrade")?;
                    if upgrade.as_deref() != Some("websocket") {
                        return Response::error("expected websocket", 400);
                    }
                    let game_canister =
                        Principal::from_text(ctx.param("game_canister").unwrap()).unwrap();
                    let token_root =
                        Principal::from_text(ctx.param("token_root").unwrap()).unwrap();
                    let user_canister =
                        Principal::from_text(ctx.param("user_canister").unwrap()).unwrap();

                    let pair = WebSocketPair::new()?;
                    ctx.data
                        .handle_ws(pair.server, game_canister, token_root, user_canister)?;

                    Response::from_websocket(pair.client)
                },
            )
            .get_async("/game_pool", |_req, ctx| async move {
                let this = ctx.data;
                let total = this.dumps().await + this.pumps().await;
                Response::ok(total.to_string())
            })
            .get("/player_count", |_req, ctx| {
                let this = ctx.data;
                let player_cnt = this.state.get_websockets().len();
                Response::ok(player_cnt.to_string())
            })
            .run(req, env)
            .await
    }

    async fn websocket_message(
        &mut self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let msg = self.handle_ws_message(&ws, message).await?;
        ws.send(&msg)?;

        Ok(())
    }

    async fn websocket_error(&mut self, ws: WebSocket, error: worker::Error) -> Result<()> {
        ws.close(Some(500), Some(error.to_string()))
    }

    async fn websocket_close(
        &mut self,
        ws: WebSocket,
        code: usize,
        reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        ws.close(Some(code as u16), Some(reason))
    }
}
