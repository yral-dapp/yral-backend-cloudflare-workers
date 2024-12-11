use crate::{platform_ochestrator::PLATFORM_ORCHESTRATOR_CANS_ID, utils::restore_impl};
use candid::Principal;
use worker::{Env, Request, Response, Result, RouteContext};

pub async fn platform_ochestrator_restore_handler(
    req: Request,
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

    platform_ochestrator_restore_impl(ctx.env).await
}

async fn platform_ochestrator_restore_impl(env: Env) -> Result<Response> {
    let platform_ochestor_cans_id = Principal::from_text(PLATFORM_ORCHESTRATOR_CANS_ID).unwrap();

    let namespace = env.durable_object("CANISTER_DATA")?;
    let stub = namespace
        .id_from_name(&platform_ochestor_cans_id.to_text())?
        .get_stub()?;

    restore_impl(platform_ochestor_cans_id, stub, &env).await
}
