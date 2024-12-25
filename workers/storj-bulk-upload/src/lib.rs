mod admin;
mod posts;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use admin::AdminCanisters;
use anyhow::Context;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use worker::{
    console_error, event, query, Context as WorkerContext, Env, Request, Response, Result,
};

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[event(fetch, respond_with_errors)]
async fn fetch(_req: Request, env: Env, _ctx: WorkerContext) -> Result<Response> {
    let admin = AdminCanisters::new(AdminCanisters::get_identity());
    let work_items = env.d1("STORJ_STAGING_DB")?;
    let low_pass = env
        .var("LOW_PASS_TIMESTAMP")?
        .to_string()
        .parse::<DateTime<Utc>>();
    let low_pass = match low_pass {
        Ok(l) => l,
        Err(err) => {
            console_error!("Couldn't parse the low pass timestamp: {err}");
            return Response::error("Couldn't parse the low pass timestamp", 500);
        }
    };

    let item_stream = posts::load_items(Arc::new(admin), low_pass)
        .await
        .context("failed to start item stream");

    let item_stream = match item_stream {
        Ok(i) => i,
        Err(err) => {
            console_error!("{err}");
            return Response::error("Failed to start item stream", 500);
        }
    };

    let total = AtomicU64::new(0);

    // the operation is io bound, so this number can be optimized to saturate
    // the network of the machine running the worker
    const CONCURRENCY_FACTOR: usize = 100;
    let res = item_stream
        .try_for_each_concurrent(CONCURRENCY_FACTOR, |item| {
            let total = &total;
            let work_items = &work_items;
            async move {
                let q = query!(
                    work_items,
                    "INSERT INTO work_items (post_id, video_id, publisher_user_id) VALUES (?1, ?2, ?3)",
                    &item.post_id,
                    &item.video_id,
                    &item.publisher_user_id,
                )
                .inspect_err(|err| console_error!("Couldn't prepare statement: {err}"))?;
                q.run()
                    .await
                    .inspect_err(|err| console_error!("Couldn't insert into db: {err}"))?;
                total.fetch_add(1, Ordering::Relaxed);
                anyhow::Ok(())
            }
        })
        .await
        .context("One of the task returned error");

    if let Err(err) = res {
        console_error!("{err}");
        return Response::error("Failed to load items", 500);
    }

    Response::ok(format!(
        "A total of {} posts were added to item store",
        total.load(Ordering::SeqCst)
    ))
}
