mod admin;
mod nsfw;
mod posts;

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use admin::AdminCanisters;
use anyhow::Context;
use chrono::{DateTime, Utc};
use futures::{StreamExt, TryStreamExt};
use nsfw::IsNsfw;
use serde_json::json;
use worker::{
    console_error, console_log, event, query, Context as WorkerContext, D1Database, Env, Request,
    Response, Result,
};

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

async fn get_item_count_in_staging(work_items: &D1Database) -> Result<usize> {
    let count = query!(
        &work_items,
        "SELECT COUNT(video_id) as count from work_items"
    )
    .all()
    .await?;
    assert!(count.success(), "query to succeed");

    // quick and dirty
    let count = count.results::<BTreeMap<String, usize>>()?;
    let count = count.first().unwrap().get("count").unwrap();

    Ok(*count)
}

#[event(fetch, respond_with_errors)]
async fn fetch(_req: Request, env: Env, _ctx: WorkerContext) -> Result<Response> {
    let admin = AdminCanisters::new(AdminCanisters::get_identity(&env)?);
    let work_items = env.d1("STORJ_STAGING_DB")?;
    let count = get_item_count_in_staging(&work_items).await?;

    console_log!("Starting out with {count} items in the d1 staging db");

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

    let added = AtomicU64::new(0);
    let skipped = AtomicU64::new(0);
    let unknown = AtomicU64::new(0);
    let maybe_nsfw = AtomicU64::new(0);

    // the operation is io bound, so this number can be optimized to saturate
    // the network of the machine running the worker
    const CONCURRENCY_FACTOR: usize = 100;
    let res = item_stream
        .try_for_each_concurrent(CONCURRENCY_FACTOR, |item| {
            let added = &added;
            let skipped = &skipped;
            let unknown = &unknown;
            let maybe_nsfw = &maybe_nsfw;
            let work_items = &work_items;
            async move {
                let q = query!(
                    work_items,
                    "INSERT OR IGNORE INTO work_items (post_id, video_id, publisher_user_id, is_nsfw) VALUES (?1, ?2, ?3, ?4)",
                    &item.post_id,
                    &item.video_id,
                    &item.publisher_user_id,
                    &item.is_nsfw.to_string()
                )
                .inspect_err(|err| console_error!("Couldn't prepare statement: {err}"))?;
                let data = q.run()
                    .await
                    .inspect_err(|err| console_error!("Couldn't insert into db: {err}"))?;
                match data.meta()?.and_then(|meta| meta.changed_db) {
                    Some(true) => {
                        added.fetch_add(1, Ordering::Relaxed);
                        if item.is_nsfw == IsNsfw::Maybe {
                            maybe_nsfw.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    Some(false) => {
                        skipped.fetch_add(1, Ordering::Relaxed);
                    },
                    None => {
                        // not too sure when this will come, so lets keep it as additional metric
                        // expect it to be zero, but if its none zero, it must be investigated
                        unknown.fetch_add(1, Ordering::Relaxed);
                    },
                }
                anyhow::Ok(())
            }
        })
        .await
        .context("One of the task returned error");

    if let Err(err) = res {
        console_error!("{err}");
        return Response::error("Failed to load items", 500);
    }

    Response::from_json(&json!({
        "added": added.load(Ordering::SeqCst),
        "skipped": skipped.load(Ordering::SeqCst),
        "unknown": unknown.load(Ordering::SeqCst),
        "maybe_nsfw": maybe_nsfw.load(Ordering::SeqCst),
        "total": {
            "before": count,
            "after": get_item_count_in_staging(&work_items).await?
        }
    }))
}
