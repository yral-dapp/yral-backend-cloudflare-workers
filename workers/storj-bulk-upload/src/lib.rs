mod admin;
mod posts;

use admin::AdminCanisters;
use futures::StreamExt;
use worker::{event, Context, Env, Request, Response, Result};

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[event(fetch)]
async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let admin = AdminCanisters::new(AdminCanisters::get_identity());

    let items: Vec<_> = posts::load_items(&admin)
        .await
        .expect("TODO: handle error when items can't be loaded")
        .collect()
        .await;

    Response::from_json(&items)
}
