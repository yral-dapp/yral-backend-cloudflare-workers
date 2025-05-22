use std::collections::HashMap;

use candid::Principal;
use hon_worker_common::{
    AirdropClaimError, ClaimRequest, GameInfo, GameInfoReq, GameRes, GameResult, HotOrNot,
    PaginatedGamesReq, PaginatedGamesRes, SatsBalanceInfo, VoteRequest, VoteRes, WithdrawRequest,
    WorkerError,
};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use std::result::Result as StdResult;
use worker::*;
use worker_utils::{
    storage::{SafeStorage, StorageCell},
    RequestInitBuilder,
};

use crate::{
    consts::{DEFAULT_ONBOARDING_REWARD_SATS, MAXIMUM_VOTE_AMOUNT_SATS},
    get_hon_game_stub_env,
    treasury::{CkBtcTreasury, CkBtcTreasuryImpl},
    treasury_obj::CkBtcTreasuryStore,
    utils::err_to_resp,
};

#[derive(Serialize, Deserialize, Clone)]
pub struct VoteRequestWithSentiment {
    pub request: VoteRequest,
    pub sentiment: HotOrNot,
    pub post_creator: Option<Principal>,
}

#[durable_object]
pub struct UserHonGameState {
    state: State,
    env: Env,
    treasury: CkBtcTreasuryImpl,
    treasury_amount: CkBtcTreasuryStore,
    sats_balance: StorageCell<BigUint>,
    airdrop_amount: StorageCell<BigUint>,
    // unix timestamp in millis, None if user has never claimed airdrop before
    last_airdrop_claimed_at: StorageCell<Option<u64>>,
    // (canister_id, post_id) -> GameInfo
    games: Option<HashMap<(Principal, u64), GameInfo>>,
}

impl UserHonGameState {
    fn storage(&self) -> SafeStorage {
        self.state.storage().into()
    }

    async fn last_airdrop_claimed_at(&mut self) -> Result<Option<u64>> {
        let storage = self.storage();
        let &last_claimed_timestamp = self.last_airdrop_claimed_at.read(&storage).await?;
        Ok(last_claimed_timestamp)
    }

    async fn claim_airdrop(
        &mut self,
        ClaimRequest {
            user_principal: _,
            amount,
        }: ClaimRequest,
    ) -> Result<StdResult<u64, AirdropClaimError>> {
        let now = Date::now().as_millis();
        let mut storage = self.storage();
        // TODO: use txns instead of separate update calls
        self.last_airdrop_claimed_at
            .update(&mut storage, |time| {
                *time = Some(now);
            })
            .await?;
        self.sats_balance
            .update(&mut storage, |balance| {
                *balance += amount;
            })
            .await?;
        self.airdrop_amount
            .update(&mut storage, |balance| {
                *balance += amount;
            })
            .await?;
        Ok(Ok(amount))
    }

    async fn games(&mut self) -> Result<&mut HashMap<(Principal, u64), GameInfo>> {
        if self.games.is_some() {
            return Ok(self.games.as_mut().unwrap());
        }

        let games = self
            .storage()
            .list_with_prefix("games-")
            .await
            .map(|v| {
                v.map(|(k, v)| {
                    let (can_raw, post_raw) =
                        k.strip_prefix("games-").unwrap().rsplit_once("-").unwrap();
                    let canister_id = Principal::from_text(can_raw).unwrap();
                    let post_id = post_raw.parse::<u64>().unwrap();
                    ((canister_id, post_id), v)
                })
            })
            .collect::<Result<_>>()?;

        self.games = Some(games);
        Ok(self.games.as_mut().unwrap())
    }

    async fn paginated_games_with_cursor(
        &mut self,
        page_size: usize,
        cursor: Option<String>,
    ) -> Result<PaginatedGamesRes> {
        let page_size = page_size.clamp(1, 100);
        let to_fetch = page_size + 1;
        let mut list_options = ListOptions::new().prefix("games-").limit(to_fetch);
        if let Some(cursor) = cursor.as_ref() {
            list_options = list_options.start(cursor.as_str());
        }

        let mut games = self
            .storage()
            .list_with_options::<GameInfo>(list_options)
            .await
            .map(|v| {
                v.map(|(k, v)| {
                    let (can_raw, post_raw) =
                        k.strip_prefix("games-").unwrap().rsplit_once("-").unwrap();
                    let canister_id = Principal::from_text(can_raw).unwrap();
                    let post_id = post_raw.parse::<u64>().unwrap();
                    GameRes {
                        post_canister: canister_id,
                        post_id,
                        game_info: v,
                    }
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next = if games.len() > page_size {
            let info = games.pop().unwrap();
            Some(format!("games-{}-{}", info.post_canister, info.post_id))
        } else {
            None
        };

        Ok(PaginatedGamesRes { games, next })
    }

    async fn redeem_sats_for_ckbtc(
        &mut self,
        user_principal: Principal,
        amount: BigUint,
    ) -> StdResult<(), (u16, WorkerError)> {
        let mut storage = self.storage();

        let mut insufficient_funds = false;
        self.sats_balance
            .update(&mut storage, |balance| {
                if *balance < amount {
                    insufficient_funds = true;
                    return;
                }
                *balance -= amount.clone();
            })
            .await
            .map_err(|_| {
                (
                    500,
                    WorkerError::Internal("failed to update balance".into()),
                )
            })?;
        if insufficient_funds {
            return Err((400, WorkerError::InsufficientFunds));
        }

        if self
            .treasury_amount
            .try_consume(&mut storage, amount.clone())
            .await
            .inspect_err(|err| {
                console_error!("withdraw error with treasury: {err:?}");
            })
            .is_err()
        {
            self.sats_balance
                .update(&mut storage, |balance| {
                    *balance += amount.clone();
                })
                .await
                .map_err(|_| {
                    (
                        500,
                        WorkerError::Internal("failed to update balance".into()),
                    )
                })?;
            return Err((400, WorkerError::TreasuryLimitReached));
        }

        if let Err(e) = self
            .treasury
            .transfer_ckbtc(user_principal, amount.clone().into())
            .await
        {
            self.treasury_amount
                .rollback(&mut storage, amount.clone())
                .await
                .map_err(|_| {
                    (
                        500,
                        WorkerError::Internal("failed to rollback treasury".into()),
                    )
                })?;
            self.sats_balance
                .update(&mut storage, |balance| {
                    *balance += amount.clone();
                })
                .await
                .map_err(|_| {
                    (
                        500,
                        WorkerError::Internal("failed to update balance".into()),
                    )
                })?;
            return Err(e);
        }

        Ok(())
    }

    async fn game_info(
        &mut self,
        post_canister: Principal,
        post_id: u64,
    ) -> Result<Option<GameInfo>> {
        let games = self.games().await?;
        Ok(games.get(&(post_canister, post_id)).cloned())
    }

    async fn add_creator_reward(&mut self, reward: u128) -> StdResult<(), (u16, WorkerError)> {
        let mut storage = self.storage();
        self.sats_balance
            .update(&mut storage, |bal| {
                *bal += reward;
            })
            .await
            .map_err(|_| {
                (
                    500,
                    WorkerError::Internal("failed to update balance".into()),
                )
            })
    }

    async fn vote_on_post(
        &mut self,
        post_canister: Principal,
        post_id: u64,
        mut vote_amount: u128,
        direction: HotOrNot,
        sentiment: HotOrNot,
        creator_principal: Option<Principal>,
    ) -> StdResult<VoteRes, (u16, WorkerError)> {
        let game_info = self
            .game_info(post_canister, post_id)
            .await
            .map_err(|_| (500, WorkerError::Internal("failed to get game info".into())))?;
        if game_info.is_some() {
            return Err((400, WorkerError::AlreadyVotedOnPost));
        }

        vote_amount = vote_amount.min(MAXIMUM_VOTE_AMOUNT_SATS);

        let mut storage = self.storage();
        let mut res = None::<(GameResult, u128)>;
        self.sats_balance
            .update(&mut storage, |balance| {
                let creator_reward = vote_amount / 10;
                let vote_amount = BigUint::from(vote_amount);
                if *balance < vote_amount {
                    return;
                }
                let game_res = if sentiment == direction {
                    let win_amt = (vote_amount.clone() * 8u32) / 10u32;
                    *balance += win_amt.clone();
                    GameResult::Win { win_amt }
                } else {
                    *balance -= vote_amount.clone();
                    GameResult::Loss {
                        lose_amt: vote_amount.clone(),
                    }
                };
                res = Some((game_res, creator_reward))
            })
            .await
            .map_err(|_| {
                (
                    500,
                    WorkerError::Internal("failed to update balance".into()),
                )
            })?;

        let Some((game_result, creator_reward)) = res else {
            return Err((400, WorkerError::InsufficientFunds));
        };

        if let Some(creator_principal) = creator_principal {
            let game_stub = get_hon_game_stub_env(&self.env, creator_principal)
                .map_err(|_| (500, WorkerError::Internal("failed to get game stub".into())))?;
            let req = Request::new_with_init(
                "http://fake_url.com/creator_reward",
                RequestInitBuilder::default()
                    .method(Method::Post)
                    .json(&creator_reward)
                    .unwrap()
                    .build(),
            )
            .expect("creator reward should build?!");
            let res = game_stub.fetch_with_request(req).await;
            if let Err(e) = res {
                eprintln!("failed to reward creator {e}");
            }
        }

        let game_info = GameInfo::Vote {
            vote_amount: BigUint::from(vote_amount),
            game_result: game_result.clone(),
        };
        self.games()
            .await
            .map_err(|_| (500, WorkerError::Internal("failed to get games".into())))?
            .insert((post_canister, post_id), game_info.clone());
        self.storage()
            .put(&format!("games-{post_canister}-{post_id}"), &game_info)
            .await
            .map_err(|_| {
                (
                    500,
                    WorkerError::Internal("failed to store game info".into()),
                )
            })?;

        Ok(VoteRes { game_result })
    }
}

#[durable_object]
impl DurableObject for UserHonGameState {
    fn new(state: State, env: Env) -> Self {
        console_error_panic_hook::set_once();

        let treasury = CkBtcTreasuryImpl::new(&env).expect("failed to create treasury");

        Self {
            state,
            env,
            treasury,
            treasury_amount: CkBtcTreasuryStore::default(),
            sats_balance: StorageCell::new("sats_balance", || {
                BigUint::from(DEFAULT_ONBOARDING_REWARD_SATS)
            }),
            airdrop_amount: StorageCell::new("airdrop_amount", || {
                BigUint::from(DEFAULT_ONBOARDING_REWARD_SATS)
            }),
            last_airdrop_claimed_at: StorageCell::new("last_airdrop_claimed_at", || None),
            games: None,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);
        router
            .post_async("/vote", async |mut req, ctx| {
                let req_data: VoteRequestWithSentiment = serde_json::from_str(&req.text().await?)?;
                let this = ctx.data;
                match this
                    .vote_on_post(
                        req_data.request.post_canister,
                        req_data.request.post_id,
                        req_data.request.vote_amount,
                        req_data.request.direction,
                        req_data.sentiment,
                        req_data.post_creator,
                    )
                    .await
                {
                    Ok(res) => Response::from_json(&res),
                    Err((code, msg)) => err_to_resp(code, msg),
                }
            })
            .get_async("/last_airdrop_claimed_at", async |_, ctx| {
                let this = ctx.data;
                let last_airdrop_claimed_at = this.last_airdrop_claimed_at().await?;

                Response::from_json(&last_airdrop_claimed_at)
            })
            .get_async("/balance", async |_, ctx| {
                let this = ctx.data;
                let storage = this.storage();
                let balance = this.sats_balance.read(&storage).await?.clone();
                let airdropped = this.airdrop_amount.read(&storage).await?.clone();
                Response::from_json(&SatsBalanceInfo {
                    balance,
                    airdropped,
                })
            })
            .post_async("/game_info", async |mut req, ctx| {
                let req_data: GameInfoReq = req.json().await?;

                let this = ctx.data;
                let game_info = this
                    .game_info(req_data.post_canister, req_data.post_id)
                    .await?;
                Response::from_json(&game_info)
            })
            .post_async("/games", async |mut req, ctx| {
                let req_data: PaginatedGamesReq = req.json().await?;
                let this = ctx.data;
                let res = this
                    .paginated_games_with_cursor(req_data.page_size, req_data.cursor)
                    .await?;

                Response::from_json(&res)
            })
            .post_async("/withdraw", async |mut req, ctx| {
                let req_data: WithdrawRequest = serde_json::from_str(&req.text().await?)?;
                let this = ctx.data;
                let res = this
                    .redeem_sats_for_ckbtc(req_data.receiver, req_data.amount.into())
                    .await;
                if let Err(e) = res {
                    return err_to_resp(e.0, e.1);
                }

                Response::ok("done")
            })
            .post_async("/claim_airdrop", async |mut req, ctx| {
                let req_data: ClaimRequest = serde_json::from_str(&req.text().await?)?;
                let this = ctx.data;
                let res = this.claim_airdrop(req_data).await?;

                match res {
                    Ok(res) => Response::ok(res.to_string()),
                    Err(e) => err_to_resp(400, e),
                }
            })
            .post_async("/creator_reward", async |mut req, ctx| {
                let amount: u128 = serde_json::from_str(&req.text().await?)?;
                let this = ctx.data;
                let res = this.add_creator_reward(amount).await;
                if let Err(e) = res {
                    return err_to_resp(e.0, e.1);
                }

                Response::ok("done")
            })
            .run(req, env)
            .await
    }
}
