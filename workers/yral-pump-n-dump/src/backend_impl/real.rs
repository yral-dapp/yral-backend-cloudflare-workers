use std::error::Error;

use candid::{Nat, Principal};
use worker::Result;
use yral_canisters_client::individual_user_template::{BalanceInfo, PumpNDumpStateDiff, Result1};

use crate::admin_cans::AdminCans;

use super::{GameBackendImpl, UserStateBackendImpl, WsBackendImpl};

fn to_worker_error(e: impl Error) -> worker::Error {
    worker::Error::RustError(e.to_string())
}

fn from_can_res(r: Result1) -> worker::Result<()> {
    match r {
        Result1::Ok => Ok(()),
        Result1::Err(e) => Err(worker::Error::RustError(e)),
    }
}

impl GameBackendImpl for AdminCans {
    async fn add_dollr_to_liquidity_pool(
        &self,
        user_canister: Principal,
        token_root: Principal,
        amount: Nat,
    ) -> Result<()> {
        let user = self.individual_user(user_canister).await;
        let res = user
            .add_dollr_to_liquidity_pool(token_root, amount)
            .await
            .map_err(to_worker_error)?;

        from_can_res(res)
    }
}

impl UserStateBackendImpl for AdminCans {
    async fn game_balance(&self, user_canister: Principal) -> Result<BalanceInfo> {
        let user = self.individual_user(user_canister).await;
        user.pd_balance_info().await.map_err(to_worker_error)
    }

    async fn reconcile_user_state(
        &self,
        user_canister: Principal,
        completed_games: Vec<PumpNDumpStateDiff>,
    ) -> Result<()> {
        let user = self.individual_user(user_canister).await;
        let res = user
            .reconcile_user_state(completed_games)
            .await
            .map_err(to_worker_error)?;

        from_can_res(res)
    }

    async fn redeem_gdollr(&self, user_canister: Principal, amount: Nat) -> Result<()> {
        let user = self.individual_user(user_canister).await;
        let res = user.redeem_gdollr(amount).await.map_err(to_worker_error)?;

        from_can_res(res)
    }

    async fn game_count(&self, user_canister: Principal) -> Result<u64> {
        let user = self.individual_user(user_canister).await;

        user.played_game_count().await.map_err(to_worker_error)
    }

    async fn net_earnings(&self, user_canister: Principal) -> Result<Nat> {
        let user = self.individual_user(user_canister).await;

        user.net_earnings().await.map_err(to_worker_error)
    }
}

impl WsBackendImpl for AdminCans {
    async fn user_principal_to_user_canister(
        &self,
        user_principal: Principal,
    ) -> Result<Option<Principal>> {
        self.user_principal_to_user_canister(user_principal).await
    }

    async fn validate_token(
        &self,
        token_root: Principal,
        token_creator: Principal,
    ) -> Result<bool> {
        let user = self.individual_user(token_creator).await;
        let tokens = user
            .deployed_cdao_canisters()
            .await
            .map_err(to_worker_error)?;

        Ok(tokens.into_iter().any(|cans| cans.root == token_root))
    }
}
