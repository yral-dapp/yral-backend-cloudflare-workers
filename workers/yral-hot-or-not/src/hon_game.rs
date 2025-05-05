use std::collections::HashMap;

use candid::{CandidType, Principal};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use std::result::Result as StdResult;
use worker::*;
use worker_utils::storage::{SafeStorage, StorageCell};

use crate::{
    consts::DEFAULT_ONBOARDING_REWARD_SATS,
    hon_sentiment_oracle::{HoNSentimentOracle, HoNSentimentOracleImpl, HotOrNot},
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GameResult {
    Win { win_amt: BigUint },
    Loss { lose_amt: BigUint },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GameInfo {
    CreatorReward(BigUint),
    Vote {
        vote_amount: BigUint,
        game_result: GameResult,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VoteRes {
    pub game_result: GameResult,
}

#[derive(Serialize, Deserialize, Clone, Debug, CandidType)]
pub struct VoteRequest {
    pub post_canister: Principal,
    pub post_id: u64,
    pub vote_amount: u128,
    pub direction: HotOrNot,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameRes {
    pub post_canister: Principal,
    pub post_id: u64,
    pub game_info: GameInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PaginatedGamesReq {
    pub page_size: usize,
    pub cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PaginatedGamesRes {
    pub games: Vec<GameRes>,
    pub next: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct GameInfoReq {
    pub post_canister: Principal,
    pub post_id: u64,
}

#[durable_object]
pub struct UserHonGameState {
    state: State,
    env: Env,
    sats_balance: StorageCell<BigUint>,
    // (canister_id, post_id) -> GameInfo
    games: Option<HashMap<(Principal, u64), GameInfo>>,
    sentiment_oracle: HoNSentimentOracle,
}

impl UserHonGameState {
    fn storage(&self) -> SafeStorage {
        self.state.storage().into()
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
                    let mut split_iter = k.strip_prefix("games-").unwrap().split("-");
                    let canister_id = Principal::from_text(split_iter.next().unwrap()).unwrap();
                    let post_id = split_iter.next().unwrap().parse::<u64>().unwrap();
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
                    let mut split_iter = k.strip_prefix("games-").unwrap().split("-");
                    let canister_id = Principal::from_text(split_iter.next().unwrap()).unwrap();
                    let post_id = split_iter.next().unwrap().parse::<u64>().unwrap();
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

    async fn game_info(
        &mut self,
        post_canister: Principal,
        post_id: u64,
    ) -> Result<Option<GameInfo>> {
        let games = self.games().await?;
        Ok(games.get(&(post_canister, post_id)).cloned())
    }

    async fn vote_on_post(
        &mut self,
        post_canister: Principal,
        post_id: u64,
        vote_amount: u128,
        direction: HotOrNot,
    ) -> StdResult<VoteRes, (u16, String)> {
        let game_info = self
            .game_info(post_canister, post_id)
            .await
            .map_err(|_| (500, "failed to get game info: internal".to_string()))?;
        if game_info.is_some() {
            return Err((400, "invalid post: already voted".to_string()));
        }

        let sentiment = self
            .sentiment_oracle
            .get_post_sentiment(post_canister, post_id)
            .await
            .map_err(|_| (500, "failed to get sentiment: internal".to_string()))?
            .ok_or_else(|| (404, "invalid post: not found".to_string()))?;

        let mut storage = self.storage();
        let mut result = None::<GameResult>;
        self.sats_balance
            .update(&mut storage, |balance| {
                let vote_amount = BigUint::from(vote_amount);
                if *balance < vote_amount {
                    return;
                }
                if sentiment == direction {
                    // TODO: add a reward for the creator
                    *balance += vote_amount.clone();
                    result = Some(GameResult::Win {
                        win_amt: vote_amount.clone(),
                    });
                } else {
                    *balance -= vote_amount.clone();
                    result = Some(GameResult::Loss {
                        lose_amt: vote_amount.clone(),
                    });
                }
            })
            .await
            .map_err(|_| (500, "failed to update balance: internal".to_string()))?;

        let Some(game_result) = result else {
            return Err((400, "insufficient balance".to_string()));
        };

        let game_info = GameInfo::Vote {
            vote_amount: BigUint::from(vote_amount),
            game_result: game_result.clone(),
        };
        self.games()
            .await
            .map_err(|_| (500, "failed to get games: internal".to_string()))?
            .insert((post_canister, post_id), game_info.clone());
        self.storage()
            .put(&format!("games-{}-{}", post_canister, post_id), &game_info)
            .await
            .map_err(|_| (500, "failed to store game info: internal".to_string()))?;

        Ok(VoteRes { game_result })
    }
}

#[durable_object]
impl DurableObject for UserHonGameState {
    fn new(state: State, env: Env) -> Self {
        console_error_panic_hook::set_once();

        let sentiment_oracle = match HoNSentimentOracle::new() {
            Ok(oracle) => oracle,
            Err(e) => panic!("failed to create sentiment oracle {e}"),
        };

        Self {
            state,
            env,
            sats_balance: StorageCell::new("sats_balance", || {
                BigUint::from(DEFAULT_ONBOARDING_REWARD_SATS)
            }),
            games: None,
            sentiment_oracle,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);
        router
            .post_async("/vote", async |mut req, ctx| {
                let req_data: VoteRequest = req.json().await?;
                let this = ctx.data;
                match this
                    .vote_on_post(
                        req_data.post_canister,
                        req_data.post_id,
                        req_data.vote_amount,
                        req_data.direction,
                    )
                    .await
                {
                    Ok(res) => Response::from_json(&res),
                    Err((code, msg)) => Response::error(msg, code),
                }
            })
            .get_async("/balance", async |_, ctx| {
                let this = ctx.data;
                let storage = this.storage();
                let balance = this.sats_balance.read(&storage).await?.clone();
                Response::ok(balance.to_string())
            })
            .get_async("/game_info", async |mut req, ctx| {
                let req_data: GameInfoReq = req.json().await?;

                let this = ctx.data;
                let game_info = this
                    .game_info(req_data.post_canister, req_data.post_id)
                    .await?;
                Response::from_json(&game_info)
            })
            .get_async("/games", async |mut req, ctx| {
                let req_data: PaginatedGamesReq = req.json().await?;
                let this = ctx.data;
                let res = this
                    .paginated_games_with_cursor(req_data.page_size, req_data.cursor)
                    .await?;

                Response::from_json(&res)
            })
            .run(req, env)
            .await
    }
}
