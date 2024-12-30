use std::collections::HashMap;

use candid::{Int, Nat, Principal};
use worker::Result;

use super::{BalanceBackendImpl, GameBackendImpl, WsBackendImpl};

pub struct MockGameBackend;

impl GameBackendImpl for MockGameBackend {
    async fn add_dollr_to_liquidity_pool(
        &mut self,
        _user_canister: Principal,
        _token_root: Principal,
        _amount: Nat,
    ) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct MockBalanceBackend(HashMap<Principal, Nat>);

const FAKE_BALANCE: u64 = 1000000000000000000;

impl BalanceBackendImpl for MockBalanceBackend {
    async fn gdollr_balance(&self, user_canister: Principal) -> Result<Nat> {
        let bal = self.0.get(&user_canister).cloned();

        Ok(bal.unwrap_or_else(|| FAKE_BALANCE.into()))
    }

    async fn settle_gdollr_balance(&mut self, user_canister: Principal, delta: Int) -> Result<()> {
        let bal = self
            .0
            .entry(user_canister)
            .or_insert_with(|| FAKE_BALANCE.into());

        if delta > 0 {
            bal.0 += delta.0.to_biguint().unwrap();
        } else {
            bal.0 -= (-delta.0).to_biguint().unwrap();
        }

        Ok(())
    }

    async fn redeem_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<()> {
        let bal = self
            .0
            .entry(user_canister)
            .or_insert_with(|| FAKE_BALANCE.into());

        *bal -= amount;

        Ok(())
    }
}

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
