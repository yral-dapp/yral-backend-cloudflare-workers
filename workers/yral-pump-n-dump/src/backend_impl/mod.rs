mod mock;
mod real;

use candid::{Int, Nat, Principal};
use enum_dispatch::enum_dispatch;
use mock::{MockBalanceBackend, MockGameBackend, MockWsBackend};
use worker::{Env, Result};

use crate::{admin_cans::AdminCans, utils::is_testing};

#[enum_dispatch]
pub(crate) trait GameBackendImpl {
    async fn add_dollr_to_liquidity_pool(
        &mut self,
        user_canister: Principal,
        token_root: Principal,
        amount: Nat,
    ) -> Result<()>;
}

#[enum_dispatch]
pub(crate) trait BalanceBackendImpl {
    async fn gdollr_balance(&self, user_canister: Principal) -> Result<Nat>;

    async fn settle_gdollr_balance(&mut self, user_canister: Principal, delta: Int) -> Result<()>;

    async fn redeem_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<()>;
}

#[enum_dispatch]
pub(crate) trait WsBackendImpl {
    async fn user_principal_to_user_canister(
        &self,
        user_principal: Principal,
    ) -> Result<Option<Principal>>;

    async fn validate_token(
        &self,
        token_root: Principal,
        token_creator_canister: Principal,
    ) -> Result<bool>;
}

#[enum_dispatch(GameBackendImpl)]
pub enum GameBackend {
    Real(AdminCans),
    Mock(MockGameBackend),
}

impl GameBackend {
    pub fn new(env: &Env) -> Result<Self> {
        if is_testing() {
            Ok(GameBackend::Mock(MockGameBackend))
        } else {
            AdminCans::new(env).map(Self::Real)
        }
    }
}

#[enum_dispatch(BalanceBackendImpl)]
pub enum BalanceBackend {
    Real(AdminCans),
    Mock(MockBalanceBackend),
}

impl BalanceBackend {
    pub fn new(env: &Env) -> Result<Self> {
        if is_testing() {
            Ok(BalanceBackend::Mock(MockBalanceBackend::default()))
        } else {
            AdminCans::new(env).map(Self::Real)
        }
    }
}

#[enum_dispatch(WsBackendImpl)]
pub enum WsBackend {
    Real(AdminCans),
    Mock(MockWsBackend),
}

impl WsBackend {
    pub fn new(env: &Env) -> Result<Self> {
        if is_testing() {
            Ok(WsBackend::Mock(MockWsBackend))
        } else {
            AdminCans::new(env).map(Self::Real)
        }
    }
}
