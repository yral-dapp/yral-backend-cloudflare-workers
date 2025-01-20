mod mock;
mod real;

use candid::{Nat, Principal};
use enum_dispatch::enum_dispatch;
use mock::{MockGameBackend, MockUserState, MockWsBackend};
use worker::{Env, Result};
use yral_canisters_client::individual_user_template::PumpNDumpStateDiff;

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
pub(crate) trait UserStateBackendImpl {
    async fn gdollr_balance(&self, user_canister: Principal) -> Result<Nat>;

    async fn withdrawable_balance(&self, user_canister: Principal) -> Result<Nat>;

    async fn reconcile_user_state(
        &mut self,
        user_canister: Principal,
        completed_games: Vec<PumpNDumpStateDiff>,
    ) -> Result<()>;

    async fn redeem_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<()>;

    async fn game_count(&self, user_canister: Principal) -> Result<u64>;
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

#[enum_dispatch(UserStateBackendImpl)]
pub enum StateBackend {
    Real(AdminCans),
    Mock(MockUserState),
}

impl StateBackend {
    pub fn new(env: &Env) -> Result<Self> {
        if is_testing() {
            Ok(StateBackend::Mock(MockUserState::default()))
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
