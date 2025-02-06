mod admin_cans;
mod agent_wrapper;
mod backend_impl;
mod consts;
mod game_object;
mod user_reconciler;
mod utils;

use backend_impl::{WsBackend, WsBackendImpl};
use candid::Principal;
use pump_n_dump_common::{
    rest::{claim_msg, ClaimReq},
    ws::identify_message,
};
use serde::{Deserialize, Serialize};
use std::result::Result as StdResult;
use user_reconciler::ClaimGdollrReq;
use utils::{game_state_stub, parse_principal, user_state_stub, RequestInitBuilder};
use worker::*;
use yral_identity::Signature;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameWsQuery {
    sender: String,
    signature: String,
}

fn verify_claim_req(req: &ClaimReq) -> StdResult<(), (String, u16)> {
    let msg = claim_msg(req.amount.clone());

    let verify_res = req.signature.clone().verify_identity(req.sender, msg);
    if verify_res.is_err() {
        return Err(("invalid signature".into(), 401));
    }

    Ok(())
}

async fn claim_gdollr(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let req: ClaimReq = req.json().await?;
    if let Err((msg, status)) = verify_claim_req(&req) {
        return Response::error(msg, status);
    }
    let backend = WsBackend::new(&ctx.env)?;

    let Some(user_canister) = backend.user_principal_to_user_canister(req.sender).await? else {
        return Response::error("user not found", 404);
    };
    let bal_stub = user_state_stub(&ctx, user_canister)?;

    let body = ClaimGdollrReq {
        user_canister,
        amount: req.amount,
    };

    let req = Request::new_with_init(
        "http://fake_url.com/claim_gdollr",
        RequestInitBuilder::default()
            .method(Method::Post)
            .json(&body)?
            .build(),
    )?;
    bal_stub.fetch_with_request(req).await?;

    Response::ok("done")
}

async fn user_balance(ctx: RouteContext<()>) -> Result<Response> {
    let user_canister = parse_principal!(ctx, "user_canister");

    let bal_stub = user_state_stub(&ctx, user_canister)?;

    let res = bal_stub
        .fetch_with_str(&format!("http://fake_url.com/balance/{user_canister}"))
        .await?;

    Ok(res)
}

async fn user_game_count(ctx: RouteContext<()>) -> Result<Response> {
    let user_canister = parse_principal!(ctx, "user_canister");

    let state_stub = user_state_stub(&ctx, user_canister)?;

    let res = state_stub
        .fetch_with_str(&format!("http://fake_url.com/game_count/{user_canister}"))
        .await?;

    Ok(res)
}

async fn user_bets_for_game(ctx: RouteContext<()>) -> Result<Response> {
    let game_canister = parse_principal!(ctx, "game_canister");
    let token_root = parse_principal!(ctx, "token_root");
    let user_canister = parse_principal!(ctx, "user_canister");

    let game_stub = game_state_stub(&ctx, game_canister, token_root)?;

    game_stub
        .fetch_with_str(&format!("http://fake_url.com/bets/{user_canister}"))
        .await
}

fn verify_identify_req(
    game_canister: Principal,
    token_root: Principal,
    sender: Principal,
    signature: Signature,
) -> StdResult<(), String> {
    let msg = identify_message(game_canister, token_root);

    let verify_res = signature.clone().verify_identity(sender, msg);
    if verify_res.is_err() {
        return Err("invalid signature".into());
    }

    Ok(())
}

async fn estabilish_game_ws(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let game_canister = parse_principal!(ctx, "game_canister");
    let token_root = parse_principal!(ctx, "token_root");

    let raw_query: GameWsQuery = req.query()?;
    let Ok(sender) = Principal::from_text(&raw_query.sender) else {
        return Response::error("invalid sender", 400);
    };
    let Ok(signature) = serde_json::from_str::<Signature>(&raw_query.signature) else {
        return Response::error("invalid signature", 400);
    };

    if let Err(e) = verify_identify_req(game_canister, token_root, sender, signature) {
        return Response::error(e, 403);
    }

    let ws_backend = WsBackend::new(&ctx.env)?;

    let Some(user_canister) = ws_backend.user_principal_to_user_canister(sender).await? else {
        return Response::error("invalid user_canister", 400);
    };

    let token_valid = ws_backend.validate_token(token_root, game_canister).await?;
    if !token_valid {
        return Response::error("invalid token", 400);
    }

    let game_stub = game_state_stub(&ctx, game_canister, token_root)?;

    let mut url = Url::parse(&format!(
        "http://fakeurl.com/ws/{game_canister}/{token_root}/{user_canister}"
    ))?;

    url.set_query(Some(&format!("sender={}", raw_query.sender)));
    url.set_query(Some(&format!("signature={}", raw_query.signature)));
    let mut headers = Headers::new();
    headers.set("Upgrade", "websocket")?;
    let new_req = Request::new_with_init(
        dbg!(url.as_str()),
        RequestInitBuilder::default()
            .method(Method::Get)
            .replace_headers(headers)
            .build(),
    )?;

    game_stub.fetch_with_request(new_req).await.inspect(|res| {
        console_log!("fetch with req: {}", res.status_code());
    })
}

async fn player_count(ctx: RouteContext<()>) -> Result<Response> {
    let game_canister = parse_principal!(ctx, "game_canister");
    let token_root = parse_principal!(ctx, "token_root");

    let game_stub = game_state_stub(&ctx, game_canister, token_root)?;

    game_stub
        .fetch_with_str("http://fake_url.com/player_count")
        .await
}

async fn net_earnings(ctx: RouteContext<()>) -> Result<Response> {
    let user_canister = parse_principal!(ctx, "user_canister");

    let state_stub = user_state_stub(&ctx, user_canister)?;

    state_stub
        .fetch_with_str(&format!("http://fake_url.com/earnings/{user_canister}"))
        .await
}

fn cors_policy() -> Cors {
    Cors::new()
        .with_origins(["*"])
        .with_methods([Method::Head, Method::Get, Method::Post, Method::Options])
        .with_allowed_headers(vec!["*"])
        .with_max_age(86400)
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    let res = router
        .post_async("/claim_gdollr", claim_gdollr)
        .get_async("/balance/:user_canister", |_req, ctx| user_balance(ctx))
        .get_async("/game_count/:user_canister", |_req, ctx| {
            user_game_count(ctx)
        })
        .get_async(
            "/bets/:game_canister/:token_root/:user_canister",
            |_req, ctx| user_bets_for_game(ctx),
        )
        .get_async("/ws/:game_canister/:token_root", |req, ctx| {
            estabilish_game_ws(req, ctx)
        })
        .get_async("/player_count/:game_canister/:token_root", |_req, ctx| {
            player_count(ctx)
        })
        .get_async("/earnings/:user_canister", |_req, ctx| net_earnings(ctx))
        .options("/*catchall", |_, _| {
            Response::empty()?.with_cors(&cors_policy())
        })
        .run(req, env)
        .await?;

    Ok(res)
}
