mod admin_cans;
mod backend_impl;
mod balance_object;
mod consts;
mod game_object;
mod utils;
mod websocket;

use backend_impl::{WsBackend, WsBackendImpl};
use balance_object::ClaimGdollrReq;
use candid::{Nat, Principal};
use serde::{Deserialize, Serialize};
use std::result::Result as StdResult;
use websocket::setup_websocket;
use worker::*;
use yral_identity::{msg_builder, Signature};

#[derive(Serialize, Deserialize, Clone)]
pub struct ClaimReq {
    // user to send DOLLR to
    pub sender: Principal,
    // amount of DOLLR
    pub amount: Nat,
    // signature asserting the user's consent
    pub signature: Signature,
}

fn verify_claim_req(req: &ClaimReq) -> StdResult<(), (String, u16)> {
    let msg = msg_builder::Message::default()
        .method_name("pump_or_dump_worker_claim".into())
        .args((req.amount.clone(),))
        .expect("Claim request should serialize");

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
    let balance_obj = ctx.durable_object("USER_DOLLR_BALANCE")?;
    let backend = WsBackend::new(&ctx.env)?;

    let Some(user_canister) = backend.user_principal_to_user_canister(req.sender).await?
    else {
        return Response::error("user not found", 404);
    };
    let user_bal_obj = balance_obj.id_from_name(&user_canister.to_text())?;
    let bal_stub = user_bal_obj.get_stub()?;

    let body = ClaimGdollrReq {
        user_canister,
        amount: req.amount,
    };
    let mut req_init = RequestInit::new();
    let req = Request::new_with_init(
        "http://fake_url.com/claim_gdollr",
        req_init
            .with_method(Method::Post)
            .with_body(Some(serde_wasm_bindgen::to_value(&body)?)),
    )?;
    bal_stub.fetch_with_request(req).await?;

    Response::ok("done")
}

async fn user_balance(ctx: RouteContext<()>) -> Result<Response> {
    let user_canister_raw = ctx.param("user_canister").unwrap();
    let Ok(user_canister) = Principal::from_text(user_canister_raw) else {
        return Response::error("Invalid user_canister", 400);
    };

    let balance_obj = ctx.durable_object("USER_DOLLR_BALANCE")?;
    let user_bal_obj = balance_obj.id_from_name(&user_canister.to_text())?;
    let bal_stub = user_bal_obj.get_stub()?;

    let res = bal_stub
        .fetch_with_str(&format!("http://fake_url.com/balance/{user_canister}"))
        .await?;

    Ok(res)
}

async fn game_status(ctx: RouteContext<()>) -> Result<Response> {
    let game_canister_raw = ctx.param("game_canister").unwrap();
    let Ok(game_canister) = Principal::from_text(game_canister_raw) else {
        return Response::error("Invalid game_canister", 400);
    };
    let token_root_raw = ctx.param("token_root").unwrap();
    let Ok(token_root) = Principal::from_text(token_root_raw) else {
        return Response::error("Invalid token_root", 400);
    };

    let game_state = ctx.durable_object("GAME_STATE")?;
    let game_state_obj =
        game_state.id_from_name(&format!("{game_canister}-{token_root}"))?;
    let game_stub = game_state_obj.get_stub()?;

    game_stub.fetch_with_str("http://fake_url.com/status").await
}

async fn user_bets_for_game(ctx: RouteContext<()>) -> Result<Response> {
    let game_canister_raw = ctx.param("game_canister").unwrap();
    let Ok(game_canister) = Principal::from_text(game_canister_raw) else {
        return Response::error("Invalid token_creator", 400);
    };
    let token_root_raw = ctx.param("token_root").unwrap();
    let Ok(token_root) = Principal::from_text(token_root_raw) else {
        return Response::error("Invalid token_root", 400);
    };
    let user_canister_raw = ctx.param("user_canister").unwrap();
    let Ok(user_canister) = Principal::from_text(user_canister_raw) else {
        return Response::error("Invalid user_canister", 400);
    };

    let game_state = ctx.durable_object("GAME_STATE")?;
    let game_state_obj =
        game_state.id_from_name(&format!("{game_canister}-{token_root}"))?;
    let game_stub = game_state_obj.get_stub()?;

    game_stub.fetch_with_str(&format!("http://fake_url.com/bets/{user_canister}")).await
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let upgrade_header = req.headers().get("Upgrade")?;
    if upgrade_header.as_deref() == Some("websocket") {
        let client = setup_websocket(env)?;

        return Ok(Response::builder()
            .with_status(101)
            .with_websocket(client)
            .empty());
    }

    let router = Router::new();

    router
        .post_async("/claim_gdollr", claim_gdollr)
        .get_async("/balance/:user_canister", |_req, ctx| user_balance(ctx))
        .get_async(
            "/bets/:game_canister/:token_root/:user_canister",
            |_req, ctx| user_bets_for_game(ctx),
        )
        .get_async(
            "/status/:game_canister/:token_root",
            |_req, ctx| game_status(ctx),
        )
        .run(req, env)
        .await
}
