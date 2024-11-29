use crate::utils::{create_agent, get_all_user_indexes, restore_impl};
use candid::Principal;
use futures::future::join_all;
use serde_json::json;
use worker::{Env, Request, Response, Result, RouteContext};

pub async fn user_index_bulk_restore_handler(
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

    let agent = create_agent(&ctx.env).await;

    let available_canisters = get_all_user_indexes(&agent).await?;

    let handles = available_canisters.into_iter().map(|cans| {
        let env = ctx.env.clone();

        async move { user_index_restore_impl(env, cans).await }
    });

    let results = join_all(handles).await;

    let mut success_count = 0;
    let mut failure_details = Vec::new();

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(_) => success_count += 1,
            Err(e) => failure_details.push(format!("Canister {}: {:?}", i + 1, e)),
        }
    }

    let response_body = json!({
        "success_count": success_count,
        "failure_count": failure_details.len(),
        "failures": failure_details,
    });

    Response::from_json(&response_body)
}

pub async fn user_index_restore_impl(env: Env, canister_id: Principal) -> Result<Response> {
    let namespace = env.durable_object("CANISTER_DATA")?;
    let stub = namespace.id_from_name(&canister_id.to_text())?.get_stub()?;

    restore_impl(canister_id, stub, &env).await
}
