use candid::{Nat, Principal};
use worker::Result;
use yral_canisters_client::individual_user_template::PumpNDumpStateDiff;

use crate::consts::GDOLLR_TO_E8S;

use super::{GameBackendImpl, UserStateBackendImpl, WsBackendImpl};

#[derive(Clone)]
pub struct NoOpGameBackend;

impl GameBackendImpl for NoOpGameBackend {
    async fn add_dollr_to_liquidity_pool(
        &self,
        _user_canister: Principal,
        _token_root: Principal,
        _amount: Nat,
    ) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct NoOpUserState;

const FAKE_BALANCE: u64 = 100 * GDOLLR_TO_E8S;

impl UserStateBackendImpl for NoOpUserState {
    async fn gdollr_balance(&self, _user_principal: Principal) -> Result<Nat> {
        Ok(FAKE_BALANCE.into())
    }

    async fn withdrawable_balance(&self, _user_canister: Principal) -> Result<Nat> {
        Ok(FAKE_BALANCE.into())
    }

    async fn reconcile_user_state(
        &self,
        _user_canister: Principal,
        _games: Vec<PumpNDumpStateDiff>,
    ) -> Result<()> {
        Ok(())
    }

    async fn redeem_gdollr(&self, _user_canister: Principal, _amount: Nat) -> Result<()> {
        Ok(())
    }

    async fn game_count(&self, _user_canister: Principal) -> Result<u64> {
        Ok(10)
    }
}

#[derive(Clone)]
pub struct MockWsBackend;

impl WsBackendImpl for MockWsBackend {
    async fn user_principal_to_user_canister(
        &self,
        user_principal: Principal,
    ) -> Result<Option<Principal>> {
        Ok(Some(user_principal))
    }

    async fn validate_token(
        &self,
        _token_root: Principal,
        _token_creator: Principal,
    ) -> Result<bool> {
        Ok(true)
    }
}
