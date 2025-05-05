use candid::Principal;
use enum_dispatch::enum_dispatch;
use hon_worker_common::HotOrNot;
use worker::{Date, Result};

#[enum_dispatch]
pub(crate) trait HoNSentimentOracleImpl {
    async fn get_post_sentiment(
        &self,
        post_canister: Principal,
        post_id: u64,
    ) -> Result<Option<HotOrNot>>;
}

#[derive(Default, Clone, Copy)]
pub struct RandomHoNSentimentOracle;

impl HoNSentimentOracleImpl for RandomHoNSentimentOracle {
    async fn get_post_sentiment(
        &self,
        _post_canister: Principal,
        _post_id: u64,
    ) -> Result<Option<HotOrNot>> {
        let time_ms = Date::now().as_millis();
        Ok(Some(if time_ms % 2 == 0 {
            HotOrNot::Hot
        } else {
            HotOrNot::Not
        }))
    }
}

#[derive(Clone)]
#[enum_dispatch(HoNSentimentOracleImpl)]
pub enum HoNSentimentOracle {
    Mock(RandomHoNSentimentOracle),
}

impl HoNSentimentOracle {
    pub fn new() -> Result<Self> {
        Ok(HoNSentimentOracle::Mock(RandomHoNSentimentOracle))
    }
}
