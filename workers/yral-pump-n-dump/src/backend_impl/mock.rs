use std::collections::HashMap;

use candid::{Nat, Principal};
use worker::Result;
use yral_canisters_client::individual_user_template::{ParticipatedGameInfo, PumpNDumpStateDiff};

use crate::consts::GDOLLR_TO_E8S;

use super::{GameBackendImpl, UserStateBackendImpl, WsBackendImpl};

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
pub struct MockUserState {
    gdollr_balances: HashMap<Principal, (Nat, Nat)>,
    games: HashMap<Principal, Vec<ParticipatedGameInfo>>,
}

const FAKE_BALANCE: u64 = 100 * GDOLLR_TO_E8S;

impl UserStateBackendImpl for MockUserState {
    async fn gdollr_balance(&self, user_canister: Principal) -> Result<Nat> {
        let bal = self.gdollr_balances.get(&user_canister).cloned();

        Ok(bal
            .map(|(game_only, withdrawable)| game_only + withdrawable)
            .unwrap_or_else(|| FAKE_BALANCE.into()))
    }

    async fn withdrawable_balance(&self, user_canister: Principal) -> Result<Nat> {
        let bal = self.gdollr_balances.get(&user_canister);

        Ok(bal.map(|b| b.1.clone()).unwrap_or_default())
    }

    async fn reconcile_user_state(
        &mut self,
        user_canister: Principal,
        games: Vec<PumpNDumpStateDiff>,
    ) -> Result<()> {
        let user_game_list = self.games.entry(user_canister).or_default();

        let mut to_deduct = 0u64;
        let mut to_add = Nat::from(0u32);
        for game in games {
            match game {
                PumpNDumpStateDiff::Participant(game) => {
                    to_deduct += game.pumps + game.dumps;
                    to_add += game.reward.clone();
                    user_game_list.push(game);
                }
                PumpNDumpStateDiff::CreatorReward(reward) => {
                    to_add += reward;
                }
            }
        }
        to_deduct *= GDOLLR_TO_E8S;

        let (game_only_bal, withdrawable_bal) = self
            .gdollr_balances
            .entry(user_canister)
            .or_insert_with(|| (FAKE_BALANCE.into(), 0u32.into()));
        *withdrawable_bal += to_add;
        if &to_deduct <= game_only_bal {
            *game_only_bal -= to_deduct;
        } else {
            let to_deduct_from_withdrawable = to_deduct - game_only_bal.clone();
            *game_only_bal = 0u32.into();
            assert!(&to_deduct_from_withdrawable <= withdrawable_bal);
            *withdrawable_bal -= to_deduct_from_withdrawable;
        }

        Ok(())
    }

    async fn redeem_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<()> {
        let (_, withdrawable_bal) = self
            .gdollr_balances
            .get_mut(&user_canister)
            .expect("trying to redeem gdollr without enough balance");

        *withdrawable_bal -= amount;

        Ok(())
    }

    async fn game_count(&self, user_canister: Principal) -> Result<u64> {
        Ok(self
            .games
            .get(&user_canister)
            .map(|g| g.len())
            .unwrap_or_default() as u64)
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
