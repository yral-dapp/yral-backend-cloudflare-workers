use candid::Principal;
use worker::{Result, RouteContext, Stub};
use worker_utils::environment::{env_kind, RunEnv};
use yral_metrics::{
    metric_sender::{
        js_spawn::JsSpawnMetricTx, mock::MaybeMockLocalMetricEventTx, vectordb::VectorDbMetricTx,
        LocalMetricTx,
    },
    metrics::EventSource,
};

pub fn game_state_stub<T>(
    ctx: &RouteContext<T>,
    game_canister: Principal,
    token_root: Principal,
) -> Result<Stub> {
    let game_ns = ctx.durable_object("GAME_STATE")?;
    let game_state_obj = game_ns.id_from_name(&format!("{game_canister}-{token_root}"))?;
    let game_stub = game_state_obj.get_stub()?;

    Ok(game_stub)
}

pub fn user_state_stub<T>(ctx: &RouteContext<T>, user_canister: Principal) -> Result<Stub> {
    let state_ns = ctx.durable_object("USER_EPHEMERAL_STATE")?;
    let state_obj = state_ns.id_from_name(&user_canister.to_text())?;

    state_obj.get_stub()
}

pub type CfMetricTx = LocalMetricTx<MaybeMockLocalMetricEventTx<JsSpawnMetricTx<VectorDbMetricTx>>>;

pub fn metrics() -> CfMetricTx {
    let ev_tx = if env_kind() == RunEnv::Remote {
        MaybeMockLocalMetricEventTx::Real(JsSpawnMetricTx(VectorDbMetricTx::default()))
    } else {
        MaybeMockLocalMetricEventTx::default()
    };

    LocalMetricTx::new(EventSource::PumpNDumpWorker, ev_tx)
}
