use std::collections::HashMap;

use candid::{Nat, Principal};
use worker::Result;
use yral_canisters_client::individual_user_template::{ParticipatedGameInfo, PumpNDumpStateDiff};

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
    gdollr_balances: HashMap<Principal, Nat>,
    games: HashMap<Principal, Vec<ParticipatedGameInfo>>,
}

const FAKE_BALANCE: u64 = 10 * (1e8 as u64);

impl UserStateBackendImpl for MockUserState {
    async fn gdollr_balance(&self, user_canister: Principal) -> Result<Nat> {
        let bal = self.gdollr_balances.get(&user_canister).cloned();

        Ok(bal.unwrap_or_else(|| FAKE_BALANCE.into()))
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

        let bal = self
            .gdollr_balances
            .entry(user_canister)
            .or_insert_with(|| FAKE_BALANCE.into());
        *bal += to_add;
        *bal -= to_deduct;

        Ok(())
    }

    async fn redeem_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<()> {
        let bal = self
            .gdollr_balances
            .entry(user_canister)
            .or_insert_with(|| FAKE_BALANCE.into());

        *bal -= amount;

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
