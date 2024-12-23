mod admin;
mod posts;

use std::sync::Arc;

use admin::AdminCanisters;
use anyhow::Context;
use futures::TryStreamExt;
use worker::{console_error, event, Context as WorkerContext, Env, Request, Response, Result};

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[event(fetch, respond_with_errors)]
async fn fetch(_req: Request, _env: Env, _ctx: WorkerContext) -> Result<Response> {
    let admin = AdminCanisters::new(AdminCanisters::get_identity());

    let items: anyhow::Result<Vec<_>> = posts::load_items(Arc::new(admin))
        .await
        .expect("TODO: handle error when items can't be loaded")
        .try_collect()
        .await
        .context("Couldn't load items");

    let items = match items {
        Ok(items) => items,
        Err(err) => {
            console_error!("{err}");
            return Response::error("Failed to load items", 500);
        }
    };

    Response::from_json(&items)
}
