use std::collections::HashSet;

use candid::{Int, Nat, Principal};
use num_bigint::BigInt;
use pump_n_dump_common::{rest::BalanceInfoResponse, GameDirection};
use serde::{Deserialize, Serialize};
use worker::*;
use yral_canisters_client::individual_user_template::{
    self, BalanceInfo, ParticipatedGameInfo, PumpNDumpStateDiff,
};

use crate::{
    backend_impl::{StateBackend, UserStateBackendImpl},
    consts::{GDOLLR_TO_E8S, USER_STATE_RECONCILE_TIME_MS},
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

    async fn try_get_user_canister(&mut self) -> Option<Principal> {
        if let Some(user_canister) = self.user_canister {
            return Some(user_canister);
        }

        let user_canister = self.state.storage().get("user_canister").await.ok()?;
        self.user_canister = Some(user_canister);

        Some(user_canister)
    }

    async fn queue_settle_balance_inner(&self) -> Result<()> {
        self.state
            .storage()
            .set_alarm(USER_STATE_RECONCILE_TIME_MS)
            .await?;

        Ok(())
    }

    async fn queue_settle_balance(&self) -> Result<()> {
        let Some(alarm) = self.state.storage().get_alarm().await? else {
            return self.queue_settle_balance_inner().await;
        };
        let new_time = Date::now().as_millis() as i64 + USER_STATE_RECONCILE_TIME_MS;
        if alarm <= new_time {
            return Ok(());
        }
        self.queue_settle_balance_inner().await?;

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
            let (key, _): (String, u32) =
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
        let on_chain_balance = self.backend.game_balance(user_canister).await?;

        Ok(self.effective_balance_inner(on_chain_balance.balance).await)
    }

    async fn effective_balance_info_inner(&mut self, mut bal_info: BalanceInfo) -> BalanceInfo {
        bal_info.balance = self.effective_balance_inner(bal_info.balance.clone()).await;

        bal_info.withdrawable = if bal_info.net_airdrop_reward > bal_info.balance {
            0u32.into()
        } else {
            bal_info.balance.clone() - bal_info.net_airdrop_reward.clone()
        };

        bal_info
    }

    async fn effective_balance_info(
        &mut self,
        user_canister: Principal,
    ) -> Result<BalanceInfoResponse> {
        let on_chain_bal = self.backend.game_balance(user_canister).await?;
        let bal_info = self.effective_balance_info_inner(on_chain_bal).await;

        Ok(BalanceInfoResponse {
            net_airdrop_reward: bal_info.net_airdrop_reward,
            balance: bal_info.balance,
            withdrawable: bal_info.withdrawable,
        })
    }

    async fn decrement(&mut self, pending_game_root: Principal) -> Result<()> {
        *self.off_chain_balance_delta().await -= GDOLLR_TO_E8S;
        self.state
            .storage()
            .put(
                "off_chain_balance_delta",
                &self.off_chain_balance_delta.as_ref().unwrap(),
            )
            .await?;

        let inserted = self.pending_games().await.insert(pending_game_root);
        if !inserted {
            return Ok(());
        }

        self.state
            .storage()
            .put(&format!("pending-game-{pending_game_root}"), 0u32)
            .await?;

        Ok(())
    }

    async fn add_state_diff_inner(&mut self, state_diff: StateDiff) -> Result<()> {
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

    async fn add_state_diff(&mut self, state_diff: StateDiff) -> Result<()> {
        self.add_state_diff_inner(state_diff).await?;
        self.queue_settle_balance().await?;

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
        let on_chain_bal = self.backend.game_balance(user_canister).await?;
        if on_chain_bal.withdrawable >= amount {
            self.backend.redeem_gdollr(user_canister, amount).await?;
            return Response::ok("done");
        }

        let effective_bal = self.effective_balance_info_inner(on_chain_bal).await;
        if amount > effective_bal.withdrawable {
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
        console_error_panic_hook::set_once();

        let backend = StateBackend::new(&env).unwrap();

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
                this.set_user_canister(user_canister).await?;
                let bal = this.effective_balance_info(user_canister).await?;
                Response::from_json(&bal)
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
                this.set_user_canister(user_canister).await?;
                let cnt = this.effective_game_count(user_canister).await?;

                Response::ok(cnt.to_string())
            })
            .run(req, env)
            .await
    }

    async fn alarm(&mut self) -> Result<Response> {
        let Some(user_canister) = self.try_get_user_canister().await else {
            console_warn!("alarm set without user_canister set?!");
            return Response::ok("not ready");
        };

        if self.state_diffs().await.is_empty() {
            console_warn!("alarm set without any updates?!");
            return Response::ok("not required");
        }

        self.settle_balance(user_canister).await?;

        Response::ok("done")
    }
}
