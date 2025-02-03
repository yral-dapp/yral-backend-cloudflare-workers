use cache::token_price::{get_token_price, types::{RequestData, TokenType}};
use serde::{Deserialize, Serialize};
use serde_json::json;
use worker::*;

pub mod cache;
mod utils;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router.get_async("/get-token-price", |mut req, ctx| async move {
        let payload = req.json::<RequestData>().await?;
        let price = get_token_price(ctx.env, payload.token_type).await?;
        Response::ok(price)
    })
    .run(req, env)
    .await
}
