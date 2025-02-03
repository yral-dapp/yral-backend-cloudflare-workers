use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum TokenType {
    BTC,
    DOLR,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RequestData {
    pub token_type: TokenType,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CachedTokenPrice {
    pub price: String,
}

#[derive(Deserialize)]
pub struct CoinbaseData {
    pub amount: String,
    pub base: String,
    pub currency: String,
}

#[derive(Deserialize)]
pub struct CoinbaseResponse {
    pub data: CoinbaseData,
}

#[derive(Deserialize)]
pub struct BinanceResponse {
    pub mins: u64,
    pub price: String,
    pub closeTime: u64,
}

#[derive(CandidType, Deserialize, Debug)]
pub struct PoolReply {
    pub tvl: candid::Nat,
    pub lp_token_symbol: String,
    pub name: String,
    pub lp_fee_0: candid::Nat,
    pub lp_fee_1: candid::Nat,
    pub balance_0: candid::Nat,
    pub balance_1: candid::Nat,
    pub rolling_24h_volume: candid::Nat,
    pub rolling_24h_apy: f64,
    pub address_0: String,
    pub address_1: String,
    pub rolling_24h_num_swaps: candid::Nat,
    pub symbol_0: String,
    pub symbol_1: String,
    pub pool_id: u32,
    pub price: f64,
    pub chain_0: String,
    pub chain_1: String,
    pub is_removed: bool,
    pub symbol: String,
    pub rolling_24h_lp_fee: candid::Nat,
    pub lp_fee_bps: u8,
}

#[derive(CandidType, Deserialize, Debug)]
pub struct PoolsReply {
    pub total_24h_lp_fee: candid::Nat,
    pub total_tvl: candid::Nat,
    pub total_24h_volume: candid::Nat,
    pub pools: Vec<PoolReply>,
    pub total_24h_num_swaps: candid::Nat,
}

#[derive(CandidType, Deserialize)]
pub enum PoolsResult {
    Ok(PoolsReply),
    Err(String),
}
