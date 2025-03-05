mod ws;

use std::{
    collections::{hash_map, HashMap},
    future::Future,
};

use crate::{
    backend_impl::{GameBackend, GameBackendImpl},
    consts::{GDOLLR_TO_E8S, TIDE_SHIFT_DELTA},
    user_reconciler::{AddRewardReq, DecrementReq, StateDiff},
    utils::{metrics, storage::SafeStorage, CfMetricTx, RequestInitBuilder},
};
use candid::{Nat, Principal};
use futures::{stream::FuturesUnordered, StreamExt};
use pump_n_dump_common::{
    rest::{CompletedGameInfo, UserBetsResponse},
    ws::{GameResult, WsResp},
    GameDirection,
};
use wasm_bindgen_futures::spawn_local;
use worker::*;
use yral_metrics::metrics::tides_turned::TidesTurned;

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
    round: Option<u64>,
    backend: GameBackend,
    metrics: CfMetricTx,
}

struct GameObjReq {
    pub sender: Principal,
    pub direction: GameDirection,
    pub creator: Principal,
    pub token_root: Principal,
    pub round: u64,
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
    fn storage(&self) -> SafeStorage {
        self.state.storage().into()
    }

    async fn pumps(&mut self) -> Result<u64> {
        if let Some(p) = self.round_pumps {
            return Ok(p);
        }

        let pumps = self.storage().get("pumps").await?.unwrap_or_default();
        self.round_pumps = Some(pumps);
        Ok(pumps)
    }

    async fn dumps(&mut self) -> Result<u64> {
        if let Some(d) = self.round_dumps {
            return Ok(d);
        }

        let dumps = self.storage().get("dumps").await?.unwrap_or_default();
        self.round_dumps = Some(dumps);
        Ok(dumps)
    }

    async fn cumulative_pumps(&mut self) -> Result<u64> {
        if let Some(tot) = self.cumulative_pumps {
            return Ok(tot);
        }

        let pumps = self.storage().get("total-pumps").await?.unwrap_or_default();
        self.cumulative_pumps = Some(pumps);
        Ok(pumps)
    }

    async fn cumulative_dumps(&mut self) -> Result<u64> {
        if let Some(tot) = self.cumulative_dumps {
            return Ok(tot);
        }

        let dumps = self.storage().get("total-dumps").await?.unwrap_or_default();
        self.cumulative_dumps = Some(dumps);
        Ok(dumps)
    }

    async fn increment_pumps_inner(&mut self) -> Result<u64> {
        let total_pumps = self.cumulative_pumps().await? + 1;
        let pumps = self.pumps().await? + 1;

        let mut storage = self.storage();
        storage.put("total-pumps", &total_pumps).await?;
        storage.put("pumps", &pumps).await?;

        self.cumulative_pumps = Some(total_pumps);
        self.round_pumps = Some(pumps);

        Ok(pumps)
    }

    async fn increment_dumps_inner(&mut self) -> Result<u64> {
        let total_dumps = self.cumulative_dumps().await? + 1;
        let dumps = self.dumps().await? + 1;

        let mut storage = self.storage();
        storage.put("total-dumps", &total_dumps).await?;
        storage.put("dumps", &dumps).await?;

        self.cumulative_dumps = Some(total_dumps);
        self.round_dumps = Some(dumps);

        Ok(dumps)
    }

    async fn has_tide_shifted(&mut self) -> Result<bool> {
        if let Some(shifted) = self.has_tide_shifted {
            return Ok(shifted);
        };

        let shifted = self
            .storage()
            .get("has_tide_shifted")
            .await?
            .unwrap_or_default();
        self.has_tide_shifted = Some(shifted);

        Ok(shifted)
    }

    async fn set_tide_shifted(&mut self) -> Result<()> {
        self.has_tide_shifted = Some(true);
        self.storage().put("has_tide_shifted", &true).await?;

        Ok(())
    }

    async fn bets(&mut self) -> Result<&mut HashMap<Principal, [u64; 2]>> {
        if self.bets.is_some() {
            return Ok(self.bets.as_mut().unwrap());
        }

        let bets = self
            .storage()
            .list_with_prefix("bets-")
            .await
            .map(|v| {
                v.map(|(k, v)| {
                    let better = Principal::from_text(k.strip_prefix("bets-").unwrap()).unwrap();
                    (better, v)
                })
            })
            .collect::<Result<_>>()?;

        self.bets = Some(bets);
        Ok(self.bets.as_mut().unwrap())
    }

    pub async fn round(&mut self) -> Result<u64> {
        if let Some(round) = self.round {
            return Ok(round);
        };

        let round = self
            .storage()
            .get("current-round")
            .await?
            .unwrap_or_default();

        self.round = Some(round);
        Ok(round)
    }

    /// advance the game round, returning the new round
    pub async fn advance_round(&mut self) -> Result<u64> {
        let new_round = self.round().await? + 1;
        self.round = Some(new_round);
        self.storage().put("current-round", &new_round).await?;

        Ok(new_round)
    }

    fn user_state_stub(&self, user: Principal) -> Result<Stub> {
        let user_state = self.env.durable_object("USER_EPHEMERAL_STATE")?;
        let user_state_obj = user_state.id_from_name(&user.to_string())?;

        user_state_obj.get_stub()
    }

    fn send_reward_to_user(
        &self,
        user: Principal,
        state_diff: StateDiff,
    ) -> Result<impl Future<Output = Result<()>> + 'static> {
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

        Ok(async move {
            user_state.fetch_with_request(req).await?;
            Ok(())
        })
    }

    async fn round_end(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
    ) -> Result<Vec<WsResp>> {
        let total_dumps = self.cumulative_dumps().await?;
        let total_pumps = self.cumulative_pumps().await?;
        let round = self.advance_round().await?;

        let pumps = self.pumps().await?;
        let dumps = self.dumps().await?;
        let rewards = RewardIter::new(
            pumps,
            dumps,
            game_creator,
            token_root,
            std::mem::take(self.bets().await?),
        );

        let winning_pool = pumps + dumps;
        // cleanup
        let mut storage = self.storage();
        storage.delete_all().await?;
        self.round_pumps = Some(0);
        self.round_dumps = Some(0);

        storage.put("total-dumps", &total_dumps).await?;
        storage.put("total-pumps", &total_pumps).await?;
        storage.put("current-round", &round).await?;

        let game_res = GameResult {
            direction: rewards.outcome,
            reward_pool: rewards.reward_pool.clone(),
            bet_count: rewards.bet_cnt,
            new_round: round,
        };

        let lp_reward = rewards.liquidity_pool.clone();

        let bets = std::mem::take(self.bets().await?);
        let mut metrics_fut = bets
            .iter()
            .map(|(winner, bet)| {
                let bet = bet.clone();
                let winner = winner.clone();
                let metrics = self.metrics.clone();
                let backend = self.backend.clone();

                async move {
                    let user_canister_details = backend
                        .user_canister_details(winner)
                        .await
                        .map_err(|e| worker::Error::RustError(e.to_string()))?;

                    let user_principal = user_canister_details.principal_id;
                    let is_registered = user_canister_details.is_registered;

                    metrics
                        .push(TidesTurned {
                            user_principal,
                            user_canister: winner,
                            is_registered,
                            staked_amount: winning_pool,
                            round_num: round,
                            user_pumps: bet[0],
                            user_dumps: bet[1],
                            round_pumps: pumps,
                            round_dumps: dumps,
                            cumulative_pumps: total_pumps,
                            cumulative_dumps: total_dumps,
                            token_root,
                        })
                        .await
                        .map_err(|e| worker::Error::RustError(e.to_string()))
                }
            })
            .collect::<FuturesUnordered<_>>();

        spawn_local(async move {
            while let Some(res) = metrics_fut.next().await {
                if let Err(e) = res {
                    console_warn!("failed to push metrics tides_turned: {e}")
                }
            }
        });

        let mut reward_futs = rewards
            .map(|(winner, reward)| self.send_reward_to_user(winner, reward))
            .collect::<Result<FuturesUnordered<_>>>()?;

        spawn_local(async move {
            while let Some(res) = reward_futs.next().await {
                if let Err(e) = res {
                    console_warn!("failed to reward user: {e}")
                }
            }
        });

        let backend = self.backend.clone();

        spawn_local(async move {
            if let Err(e) = backend
                .add_dollr_to_liquidity_pool(game_creator, token_root, lp_reward)
                .await
            {
                console_warn!("failed to add reward to liquidity pool: {e}");
            };
        });

        Ok(vec![
            WsResp::BetSuccesful { round: round - 1 },
            WsResp::WinningPoolEvent {
                new_pool: winning_pool,
                round: round - 1,
            },
            WsResp::GameResultEvent(game_res),
        ])
    }

    async fn tide_shift_check(&mut self, with: u64, other: u64) -> Result<bool> {
        let prev_delta = (with - 1).saturating_sub(other);
        let new_delta = (with).saturating_sub(other);

        let shifted = prev_delta < TIDE_SHIFT_DELTA && new_delta >= TIDE_SHIFT_DELTA;
        if !shifted {
            return Ok(false);
        }

        if !self.has_tide_shifted().await? {
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
    ) -> Result<Vec<WsResp>> {
        let bets = self.bets().await?.entry(sender).or_insert([0, 0]);
        bets[0] += 1;
        let bets = *bets;

        self.increment_pumps_inner().await?;

        let total_pumps = self.cumulative_pumps().await?;
        let total_dumps = self.cumulative_dumps().await?;
        let tide_shifted = self.tide_shift_check(total_pumps, total_dumps).await?;

        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.storage().put(&format!("bets-{sender}"), &bets).await?;

        let round = self.round().await?;
        let pool = self.pumps().await? + self.dumps().await?;

        Ok(vec![
            WsResp::BetSuccesful { round },
            WsResp::WinningPoolEvent {
                round,
                new_pool: pool,
            },
        ])
    }

    async fn increment_dumps(
        &mut self,
        game_creator: Principal,
        token_root: Principal,
        sender: Principal,
    ) -> Result<Vec<WsResp>> {
        let bets = self.bets().await?.entry(sender).or_insert([0, 0]);
        bets[1] += 1;
        let bets = *bets;

        self.increment_dumps_inner().await?;

        let total_pumps = self.cumulative_pumps().await?;
        let total_dumps = self.cumulative_dumps().await?;
        let tide_shifted = self.tide_shift_check(total_dumps, total_pumps).await?;

        if tide_shifted {
            return self.round_end(game_creator, token_root).await;
        }

        self.storage().put(&format!("bets-{sender}"), &bets).await?;

        let round = self.round().await?;
        let pool = self.pumps().await? + self.dumps().await?;

        Ok(vec![
            WsResp::BetSuccesful { round },
            WsResp::WinningPoolEvent {
                new_pool: pool,
                round,
            },
        ])
    }

    async fn game_request(&mut self, game_req: GameObjReq) -> Result<Vec<WsResp>> {
        if self.round().await? != game_req.round {
            return Err(Error::RustError("round mismatch".into()));
        }

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

        let mut res = user_state.fetch_with_request(req).await?;
        if res.status_code() != 200 {
            return Err(worker::Error::RustError(res.text().await.unwrap()));
        }

        match game_req.direction {
            GameDirection::Pump => {
                self.increment_pumps(game_req.creator, game_req.token_root, game_req.sender)
                    .await
            }
            GameDirection::Dump => {
                self.increment_dumps(game_req.creator, game_req.token_root, game_req.sender)
                    .await
            }
        }
    }
}

#[durable_object]
impl DurableObject for GameState {
    fn new(state: State, env: Env) -> Self {
        console_error_panic_hook::set_once();

        let backend = match GameBackend::new(&env) {
            Ok(b) => b,
            Err(e) => panic!("Failed to create backend: {e}"),
        };

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
            round: None,
            metrics: metrics(),
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
                    .await?
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
                        .handle_ws(pair.server, game_canister, token_root, user_canister)
                        .await?;

                    Response::from_websocket(pair.client)
                },
            )
            .get_async("/game_pool", |_req, ctx| async move {
                let this = ctx.data;
                let total = this.dumps().await? + this.pumps().await?;
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
        self.handle_ws_message(&ws, message).await?;

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
