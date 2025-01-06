use std::collections::HashSet;

use candid::{Int, Nat, Principal};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use worker::*;
use yral_canisters_client::individual_user_template::{
    self, ParticipatedGameInfo, PumpNDumpStateDiff,
};

use crate::{
    backend_impl::{StateBackend, UserStateBackendImpl},
    consts::GDOLLR_TO_E8S,
    websocket::GameDirection,
};

#[derive(Serialize, Deserialize, Clone)]
pub struct AddRewardReq {
    pub state_diff: StateDiff,
    pub user_canister: Principal,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DecrementReq {
    pub user_canister: Principal,
    pub token_root: Principal,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClaimGdollrReq {
    pub user_canister: Principal,
    pub amount: Nat,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CompletedGameInfo {
    pub pumps: u64,
    pub dumps: u64,
    pub reward: Nat,
    pub token_root: Principal,
    pub outcome: GameDirection,
}

impl From<CompletedGameInfo> for ParticipatedGameInfo {
    fn from(value: CompletedGameInfo) -> Self {
        Self {
            pumps: value.pumps,
            game_direction: if matches!(value.outcome, GameDirection::Pump) {
                individual_user_template::GameDirection::Pump
            } else {
                individual_user_template::GameDirection::Dump
            },
            reward: value.reward,
            dumps: value.dumps,
            token_root: value.token_root,
        }
    }
}

impl From<ParticipatedGameInfo> for CompletedGameInfo {
    fn from(value: ParticipatedGameInfo) -> Self {
        Self {
            pumps: value.pumps,
            dumps: value.dumps,
            reward: value.reward,
            token_root: value.token_root,
            outcome: if matches!(
                value.game_direction,
                individual_user_template::GameDirection::Pump
            ) {
                GameDirection::Pump
            } else {
                GameDirection::Dump
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum StateDiff {
    CompletedGame(CompletedGameInfo),
    CreatorReward(Nat),
}

impl From<StateDiff> for PumpNDumpStateDiff {
    fn from(value: StateDiff) -> Self {
        match value {
            StateDiff::CompletedGame(info) => Self::Participant(info.into()),
            StateDiff::CreatorReward(reward) => Self::CreatorReward(reward),
        }
    }
}

impl StateDiff {
    pub fn reward(&self) -> Nat {
        match self {
            Self::CompletedGame(info) => info.reward.clone(),
            Self::CreatorReward(reward) => reward.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum GameInfo {
    Completed(CompletedGameInfo),
    Pending { token_root: Principal },
}

#[durable_object]
pub struct UserEphemeralState {
    state: State,
    env: Env,
    // effective balance = on_chain_balance + off_chain_balance_delta
    off_chain_balance_delta: Option<Int>,
    user_canister: Option<Principal>,
    state_diffs: Option<Vec<StateDiff>>,
    pending_games: Option<HashSet<Principal>>,
    backend: StateBackend,
}

impl UserEphemeralState {
    async fn set_user_canister(&mut self, user_canister: Principal) -> Result<()> {
        if self.user_canister.is_some() {
            return Ok(());
        }

        self.user_canister = Some(user_canister);
        self.state
            .storage()
            .put("user_canister", user_canister)
            .await?;

        Ok(())
    }

    async fn off_chain_balance_delta(&mut self) -> &mut Int {
        // if let Some syntax causes lifetime issues
        if self.off_chain_balance_delta.is_some() {
            return self.off_chain_balance_delta.as_mut().unwrap();
        }

        let off_chain_balance_delta = self
            .state
            .storage()
            .get("off_chain_balance_delta")
            .await
            .unwrap_or_default();
        self.off_chain_balance_delta = Some(off_chain_balance_delta);
        self.off_chain_balance_delta.as_mut().unwrap()
    }

    async fn pending_games(&mut self) -> &mut HashSet<Principal> {
        if self.pending_games.is_some() {
            return self.pending_games.as_mut().unwrap();
        }

        let pending_games_idx = self
            .state
            .storage()
            .list_with_options(ListOptions::new().prefix("pending-game-"))
            .await
            .unwrap_or_default();

        let mut pending_games = HashSet::new();
        for entry in pending_games_idx.entries() {
            let raw_entry = entry.expect("invalid pending games stored?!");
            let (key, _): (String, ()) =
                serde_wasm_bindgen::from_value(raw_entry).expect("invalid pending games stored?!");
            let pending_game =
                Principal::from_text(key.strip_prefix("pending-game-").unwrap()).unwrap();
            pending_games.insert(pending_game);
        }

        self.pending_games = Some(pending_games);
        self.pending_games.as_mut().unwrap()
    }

    async fn state_diffs(&mut self) -> &mut Vec<StateDiff> {
        if self.state_diffs.is_some() {
            return self.state_diffs.as_mut().unwrap();
        }

        let state_diff_idx = self
            .state
            .storage()
            .list_with_options(ListOptions::new().prefix("state-diff-"))
            .await
            .unwrap_or_default();

        let mut state_diffs = Vec::with_capacity(state_diff_idx.size() as usize);
        for entry in state_diff_idx.entries() {
            let raw_entry = entry.expect("invalid state diff stored?!");
            let (_, state_diff): (String, StateDiff) =
                serde_wasm_bindgen::from_value(raw_entry).expect("invalid state diff stored?!");
            state_diffs.push(state_diff);
        }

        self.state_diffs = Some(state_diffs);
        self.state_diffs.as_mut().unwrap()
    }

    async fn effective_balance_inner(&mut self, on_chain_balance: Nat) -> Nat {
        let mut effective_balance = on_chain_balance;
        let off_chain_delta = self.off_chain_balance_delta().await.clone();
        if off_chain_delta < 0 {
            effective_balance.0 -= (-off_chain_delta.0.clone()).to_biguint().unwrap();
        } else {
            effective_balance.0 += off_chain_delta.0.to_biguint().unwrap();
        };

        effective_balance
    }

    async fn effective_balance(&mut self, user_canister: Principal) -> Result<Nat> {
        let on_chain_balance = self.backend.gdollr_balance(user_canister).await?;

        Ok(self.effective_balance_inner(on_chain_balance).await)
    }

    async fn decrement(&mut self, pending_game_root: Principal) -> Result<()> {
        *self.off_chain_balance_delta().await -= GDOLLR_TO_E8S;
        self.state
            .storage()
            .put(
                "off_chain_balance_delta",
                self.off_chain_balance_delta.clone(),
            )
            .await?;

        let inserted = self.pending_games().await.insert(pending_game_root);
        if !inserted {
            return Ok(());
        }

        self.state
            .storage()
            .put(&format!("pending-game-{pending_game_root}"), ())
            .await?;

        Ok(())
    }

    async fn add_state_diff(&mut self, state_diff: StateDiff) -> Result<()> {
        self.off_chain_balance_delta().await.0 += BigInt::from(state_diff.reward());
        self.state
            .storage()
            .put(
                "off_chain_balance_delta",
                self.off_chain_balance_delta.clone(),
            )
            .await?;

        let state_diffs = self.state_diffs().await;
        state_diffs.push(state_diff.clone());
        let next_idx = state_diffs.len() - 1;

        if let StateDiff::CompletedGame(ginfo) = &state_diff {
            self.pending_games().await.remove(&ginfo.token_root);
            self.state
                .storage()
                .delete(&format!("pending-game-{}", ginfo.token_root))
                .await?;
        }

        self.state
            .storage()
            .put(&format!("state-diff-{}", next_idx), state_diff)
            .await?;

        Ok(())
    }

    async fn settle_balance(&mut self, user_canister: Principal) -> Result<()> {
        let to_settle = self.off_chain_balance_delta().await.clone();
        self.off_chain_balance_delta = Some(0.into());
        self.state
            .storage()
            .put("off_chain_balance_delta", Nat::from(0u32))
            .await?;

        let state_diffs = std::mem::take(self.state_diffs().await);
        self.state
            .storage()
            .delete_multiple(
                (0..state_diffs.len())
                    .map(|i| format!("state-diff-{i}"))
                    .collect(),
            )
            .await?;

        let res = self
            .backend
            .reconcile_user_state(
                user_canister,
                state_diffs.iter().cloned().map(Into::into).collect(),
            )
            .await;

        if let Err(e) = res {
            self.off_chain_balance_delta = Some(to_settle.clone());
            self.state_diffs = Some(state_diffs.clone());
            self.state
                .storage()
                .put("off_chain_balance_delta", to_settle)
                .await?;
            for (i, state_diff) in state_diffs.into_iter().enumerate() {
                self.state
                    .storage()
                    .put(&format!("state-diff-{i}"), state_diff)
                    .await?;
            }

            return Err(e);
        }

        Ok(())
    }

    async fn claim_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<Response> {
        let on_chain_bal = self.backend.gdollr_balance(user_canister).await?;
        if on_chain_bal >= amount {
            self.backend.redeem_gdollr(user_canister, amount).await?;
            return Response::ok("done");
        }

        let effective_bal = self.effective_balance_inner(on_chain_bal).await;
        if amount > effective_bal {
            return Response::error("not enough balance", 400);
        }

        self.settle_balance(user_canister).await?;
        self.backend.redeem_gdollr(user_canister, amount).await?;

        Response::ok("done")
    }

    async fn effective_game_count(&mut self, user_canister: Principal) -> Result<u64> {
        let on_chain_count = self.backend.game_count(user_canister).await?;
        let off_chain_count = self.state_diffs().await.len() + self.pending_games().await.len();

        Ok(on_chain_count + off_chain_count as u64)
    }
}

#[durable_object]
impl DurableObject for UserEphemeralState {
    fn new(state: State, env: Env) -> Self {
        let backend = StateBackend::new(&env).unwrap();

        // TODO: do we need balance flushing?
        Self {
            state,
            env,
            off_chain_balance_delta: None,
            user_canister: None,
            state_diffs: None,
            pending_games: None,
            backend,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);

        router
            .get_async("/balance/:user_canister", |_req, ctx| async {
                let user_canister_raw = ctx.param("user_canister").unwrap();
                let Ok(user_canister) = Principal::from_text(user_canister_raw) else {
                    return Response::error("Invalid user_canister", 400);
                };

                let this = ctx.data;
                let bal = this.effective_balance(user_canister).await?;
                Response::ok(bal.to_string())
            })
            .post_async("/decrement", |mut req, ctx| async move {
                let this = ctx.data;
                let decr_req: DecrementReq = req.json().await?;
                this.set_user_canister(decr_req.user_canister).await?;

                let bal = this.effective_balance(decr_req.user_canister).await?;
                if bal < GDOLLR_TO_E8S {
                    return Response::error("Not enough balance", 400);
                }
                this.decrement(decr_req.token_root).await?;

                Response::ok("done")
            })
            .post_async("/add_reward", |mut req, ctx| async move {
                let this = ctx.data;
                let reward_req: AddRewardReq = req.json().await?;

                this.set_user_canister(reward_req.user_canister).await?;
                this.add_state_diff(reward_req.state_diff).await?;

                Response::ok("done")
            })
            .post_async("/claim_gdollr", |mut req, ctx| async move {
                let this = ctx.data;
                let claim_req: ClaimGdollrReq = req.json().await?;

                this.set_user_canister(claim_req.user_canister).await?;

                this.claim_gdollr(claim_req.user_canister, claim_req.amount)
                    .await
            })
            .get_async("/game_count/:user_canister", |_req, ctx| async move {
                let user_canister_raw = ctx.param("user_canister").unwrap();
                let Ok(user_canister) = Principal::from_text(user_canister_raw) else {
                    return Response::error("Invalid user_canister", 400);
                };

                let this = ctx.data;
                let cnt = this.effective_game_count(user_canister).await?;

                Response::ok(cnt.to_string())
            })
            .run(req, env)
            .await
    }
}
