use crate::utils::backup_impl;
use candid::Principal;
use ic_agent::Agent;
use worker::{Env, Request, Response, Result, RouteContext};

use crate::{
    utils::{create_agent, get_user_principal_from_canister},
    RequestData,
};

pub async fn individual_user_backup_handler(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let auth_header = req.headers().get("AUTH_TOKEN")?.unwrap_or_default();
    if auth_header
        != ctx
            .secret("CF_WORKER_ACCESS_OFF_CHAIN_AGENT_KEY")
            .unwrap()
            .to_string()
    {
        return Response::error("Incorrect Auth Token", 403);
    }

    let agent = create_agent(&ctx.env).await;

    let RequestData { canister_id } = req.json().await?;

    individual_user_backup_impl(ctx.env, agent, canister_id).await
}

pub async fn individual_user_backup_impl(
    env: Env,
    agent: Agent,
    canister_id: Principal,
) -> Result<Response> {
    let Some(principal_id) = get_user_principal_from_canister(&agent, canister_id).await? else {
        return Response::error("User Canister not found in User Index", 404);
    };
    let namespace = env.durable_object("CANISTER_DATA")?;
    let stub = namespace
        .id_from_name(&principal_id.to_text())?
        .get_stub()?;

    backup_impl(&agent, canister_id, stub, &env).await
}
