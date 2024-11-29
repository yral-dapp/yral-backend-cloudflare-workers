use crate::{platform_ochestrator::PLATFORM_ORCHESTRATOR_CANS_ID, utils::backup_impl};
use candid::Principal;
use worker::{Env, Response, Result};

use crate::utils::create_agent;

pub async fn platform_ochestrator_backup(env: &Env) -> Result<Response> {
    let agent = create_agent(env).await;

    let platform_ochestor_cans_id = Principal::from_text(PLATFORM_ORCHESTRATOR_CANS_ID).unwrap();

    let namespace = env.durable_object("CANISTER_DATA")?;
    let stub = namespace
        .id_from_name(&platform_ochestor_cans_id.to_text())?
        .get_stub()?;

    backup_impl(&agent, platform_ochestor_cans_id, stub, env).await
}
