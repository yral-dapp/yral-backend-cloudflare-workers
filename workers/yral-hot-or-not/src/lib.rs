mod consts;
mod hon_game;
mod hon_sentiment_oracle;

use candid::Principal;
use hon_worker_common::{hon_game_vote_msg, GameInfoReq, HoNGameVoteReq, PaginatedGamesReq};
use std::result::Result as StdResult;
use worker::*;
use worker_utils::{parse_principal, RequestInitBuilder};

fn cors_policy() -> Cors {
    Cors::new()
        .with_origins(["*"])
        .with_methods([Method::Head, Method::Get, Method::Post, Method::Options])
        .with_allowed_headers(vec!["*"])
        .with_max_age(86400)
}

fn verify_hon_game_req(sender: Principal, req: &HoNGameVoteReq) -> StdResult<(), (u16, String)> {
    let msg = hon_game_vote_msg(req.request.clone());

    req.signature
        .clone()
        .verify_identity(sender, msg)
        .map_err(|_| (401, "invalid signature".into()))?;

    Ok(())
}

fn get_hon_game_stub(ctx: &RouteContext<()>, user_principal: Principal) -> Result<Stub> {
    let game_ns = ctx.durable_object("USER_HON_GAME_STATE")?;
    let game_state_obj = game_ns.id_from_name(&user_principal.to_text())?;
    let game_stub = game_state_obj.get_stub()?;

    Ok(game_stub)
}

async fn place_hot_or_not_vote(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_principal = parse_principal!(ctx, "user_principal");

    let req = req.json::<HoNGameVoteReq>().await?;
    if let Err((code, err)) = verify_hon_game_req(user_principal, &req) {
        return Response::error(err, code);
    };

    let game_stub = get_hon_game_stub(&ctx, user_principal)?;

    let req = Request::new_with_init(
        "http://fake_url.com/vote",
        RequestInitBuilder::default()
            .method(Method::Post)
            .json(&req.request)?
            .build(),
    )?;

    let res = game_stub.fetch_with_request(req).await?;

    Ok(res)
}

async fn user_sats_balance(ctx: RouteContext<()>) -> Result<Response> {
    let user_principal = parse_principal!(ctx, "user_principal");

    let game_stub = get_hon_game_stub(&ctx, user_principal)?;

    let res = game_stub
        .fetch_with_str("http://fake_url.com/balance")
        .await?;

    Ok(res)
}

async fn game_info(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_principal = parse_principal!(ctx, "user_principal");

    let game_stub = get_hon_game_stub(&ctx, user_principal)?;

    let req_data: GameInfoReq = req.json().await?;

    let req = Request::new_with_init(
        "http://fake_url.com/game_info",
        RequestInitBuilder::default()
            .method(Method::Get)
            .json(&req_data)?
            .build(),
    )?;

    let res = game_stub.fetch_with_request(req).await?;

    Ok(res)
}

async fn paginated_games(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_principal = parse_principal!(ctx, "user_principal");

    let game_stub = get_hon_game_stub(&ctx, user_principal)?;

    let req_data: PaginatedGamesReq = req.json().await?;

    let req = Request::new_with_init(
        "http://fake_url.com/games",
        RequestInitBuilder::default()
            .method(Method::Get)
            .json(&req_data)?
            .build(),
    )?;

    let res = game_stub.fetch_with_request(req).await?;

    Ok(res)
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    let res = router
        .get_async("/balance/:user_principal", |_req, ctx| {
            user_sats_balance(ctx)
        })
        .get_async("/game_info/:user_principal", game_info)
        .get_async("/games/:user_principal", |req, ctx| {
            paginated_games(req, ctx)
        })
        .post_async("/vote/:user_principal", |req, ctx| {
            place_hot_or_not_vote(req, ctx)
        })
        .run(req, env)
        .await?;

    res.with_cors(&cors_policy())
}
