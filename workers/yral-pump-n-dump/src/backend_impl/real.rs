use std::error::Error;

use candid::{Int, Nat, Principal};
use worker::Result;
use yral_canisters_client::individual_user_template::Result1;

use crate::admin_cans::AdminCans;

use super::{BalanceBackendImpl, GameBackendImpl, WsBackendImpl};

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
        &mut self,
        user_canister: Principal,
        token_root: Principal,
        amount: Nat,
    ) -> Result<()> {
        let user = self.individual_user(user_canister);
        let res = user
            .add_dollr_to_liquidity_pool(token_root, amount)
            .await
            .map_err(to_worker_error)?;

        from_can_res(res)
    }
}

impl BalanceBackendImpl for AdminCans {
    async fn gdollr_balance(&self, user_canister: Principal) -> Result<Nat> {
        let user = self.individual_user(user_canister);
        user.gdollr_balance().await.map_err(to_worker_error)
    }

    async fn settle_gdollr_balance(&mut self, user_canister: Principal, delta: Int) -> Result<()> {
        let user = self.individual_user(user_canister);
        let res = user
            .settle_gdollr_balance(delta)
            .await
            .map_err(to_worker_error)?;

        from_can_res(res)
    }

    async fn redeem_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<()> {
        let user = self.individual_user(user_canister);
        let res = user.redeem_gdollr(amount).await.map_err(to_worker_error)?;

        from_can_res(res)
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
        let user = self.individual_user(token_creator);
        let tokens = user
            .deployed_cdao_canisters()
            .await
            .map_err(to_worker_error)?;

        Ok(tokens.into_iter().any(|cans| cans.root == token_root))
    }
}
