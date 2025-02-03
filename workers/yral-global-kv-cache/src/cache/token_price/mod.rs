pub mod types;
use std::str::FromStr;

use bigdecimal::BigDecimal;
use candid::{encode_args, Decode, Principal};
use ic_agent::Agent;
use serde_json;
use worker::*;

use types::*;

const TTL: u64 = 5 * 60;

async fn fetch_coinbase_price(token_type: TokenType) -> Result<String> {
    let mut req_init = RequestInit::new();
    req_init.with_method(Method::Get);
    let url = format!(
        "https://api.coinbase.com/v2/prices/{:?}-USD/spot",
        token_type
    );
    let req = Request::new_with_init(&url, &req_init)?;
    let mut resp = Fetch::Request(req).send().await?;
    let body = resp.text().await?;
    let coinbase_response: CoinbaseResponse = serde_json::from_str(&body)?;
    let price = coinbase_response.data.amount;
    Ok(price)
}

async fn fetch_binance_price(token_type: TokenType) -> Result<String> {
    let mut req_init = RequestInit::new();
    req_init.with_method(Method::Get);
    let url = format!(
        "https://api.binance.com/api/v3/avgPrice?symbol={:?}USDC",
        token_type
    );
    let req = Request::new_with_init(&url, &req_init)?;
    let mut resp = Fetch::Request(req).send().await?;
    let body = resp.text().await?;

    let binance_response: BinanceResponse = serde_json::from_str(&body)?;
    let price = binance_response.price;
    Ok(price)
}

pub async fn fetch_token_price_from_kong_swap(token_name: String) -> Result<f64> {
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .build()
        .unwrap();

    let pools_result = agent
        .query(
            &Principal::from_text("cbefx-hqaaa-aaaar-qakrq-cai")
                .map_err(|e| worker::Error::from(e.to_string()))?,
            "pools",
        )
        .with_arg(encode_args((Some(token_name),)).map_err(|e| worker::Error::from(e.to_string()))?)
        .call()
        .await
        .map_err(|e| worker::Error::from(e.to_string()))?;

    let pools_result: PoolsResult =
        Decode!(&pools_result, PoolsResult).map_err(|e| worker::Error::from(e.to_string()))?;

    match pools_result {
        PoolsResult::Ok(pools_reply) => {
            let usdc_pool = pools_reply
                .pools
                .iter()
                .find(|pool| pool.symbol_1 == "ckUSDC" || pool.symbol_1 == "ckUSDT");

            match usdc_pool {
                Some(pool) => Ok(pool.price),
                None => Err(worker::Error::from("No USDC/USDT pool found")),
            }
        }
        PoolsResult::Err(e) => Err(worker::Error::from(e)),
    }
}

pub async fn get_token_price(env: Env, token_type: TokenType) -> Result<String> {
    let kv = env.kv("yral-backend-application-cache")?;
    let key = format!("{:?}-USD", token_type);

    if let Some(cached) = kv.get(&key).json::<CachedTokenPrice>().await? {
        return Ok(cached.price);
    }

    match token_type {
        TokenType::BTC => {
            let (coinbase_result, binance_result) = (fetch_coinbase_price(token_type).await,
            fetch_binance_price(token_type).await);

            let price = match (coinbase_result, binance_result) {
                (Ok(cb_price), Ok(bb_price)) => {
                    let cb_val = BigDecimal::from_str(&cb_price).map_err(|e| {
                        worker::Error::from(format!("Failed to parse Coinbase price: {}", e))
                    })?;
                    let bb_val = BigDecimal::from_str(&bb_price).map_err(|e| {
                        worker::Error::from(format!("Failed to parse Binance price: {}", e))
                    })?;
                    let avg_val = (cb_val + bb_val) / BigDecimal::from(2);
                    avg_val.to_string()
                }
                (Ok(cb_price), Err(_)) => cb_price,
                (Err(_), Ok(bb_price)) => bb_price,
                (Err(e), Err(_)) => return Err(e),
            };

            kv.put(
                &key,
                &CachedTokenPrice {
                    price: price.clone(),
                },
            )?
            .expiration_ttl(TTL)
            .execute()
            .await?;

            Ok(price)
        }
        TokenType::DOLR => {
            let price = fetch_token_price_from_kong_swap("DOLR".to_string())
                .await?
                .to_string();
            kv.put(
                &key,
                &CachedTokenPrice {
                    price: price.clone(),
                },
            )?
            .expiration_ttl(TTL)
            .execute()
            .await?;

            Ok(price)
        }
    }
}
