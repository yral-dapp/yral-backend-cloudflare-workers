use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Utc};
use futures::{future, stream, StreamExt, TryStreamExt};
use worker::{console_error, console_log};
use yral_canisters_client::individual_user_template::{
    GetPostsOfUserProfileError, IndividualUserTemplate, PostDetailsForFrontend,
};

use crate::admin::AdminCanisters;

#[derive(Debug, serde::Deserialize, serde::Serialize, Default)]
pub(crate) struct Item {
    pub(crate) video_id: String,
    pub(crate) publisher_user_id: String,
    pub(crate) post_id: u64,
    // TODO: extra metadata
}

/// loads all posts for the given user and buffers into a vec before returning
async fn load_all_posts(
    user: &IndividualUserTemplate<'_>,
    low_pass: DateTime<Utc>,
) -> anyhow::Result<Vec<PostDetailsForFrontend>> {
    const LIMIT: usize = 100;
    let mut posts = Vec::new();

    for page in (0..).step_by(LIMIT) {
        let post_res = user
            .get_posts_of_this_user_profile_with_pagination_cursor(page, LIMIT as u64)
            .await
            .context("Couldn't get post")?;

        use yral_canisters_client::individual_user_template::Result12;
        let post = match post_res {
            Result12::Ok(posts) => posts,
            Result12::Err(GetPostsOfUserProfileError::ReachedEndOfItemsList) => break,
            Result12::Err(err) => anyhow::bail!("{err:?}"),
        };

        posts.extend(post.into_iter())
    }

    posts.retain(|post| {
        let created_at = DateTime::from_timestamp_nanos(post.created_at.nanos_since_epoch as i64);

        // MUST BE NON-INCLUSIVE
        created_at < low_pass
    });

    Ok(posts)
}

pub(crate) async fn load_items<'a>(
    admin: Arc<AdminCanisters>,
    low_pass: DateTime<Utc>,
) -> anyhow::Result<impl futures::Stream<Item = anyhow::Result<Item>>> {
    let subs = admin
        .platform_orchestrator()
        .await
        .get_all_subnet_orchestrators()
        .await
        .context("Couldn't fetch the subnet orchestrator")?;

    let admin_for_index = admin.clone();
    let admin_for_individual_user = admin.clone();
    let items = stream::iter(subs)
        .then(move |sub| {
            let admin = admin_for_index.clone();
            async move {
                admin
                    .user_index_with(sub)
                    .await
                    .get_user_canister_list()
                    .await
                    .inspect(|users| console_log!("found {} users", users.len()))
                    .inspect_err(|err| console_error!("{err}"))
            }
        })
        .and_then(|list| future::ok(stream::iter(list).map(anyhow::Ok)))
        .try_flatten_unordered(None)
        .and_then(move |user_principal| {
            let admin = admin_for_individual_user.clone();
            async move {
                let index = admin.individual_user_for(user_principal).await;
                load_all_posts(&index, low_pass)
                    .await
                    .inspect(|posts| {
                        console_log!("found {} posts for {}", posts.len(), user_principal)
                    })
                    .inspect_err(|err| console_error!("{err}"))
            }
        })
        .and_then(|list| future::ok(stream::iter(list).map(anyhow::Ok)))
        .try_flatten_unordered(None)
        .map(|post| {
            post.map(|post| Item {
                video_id: post.video_uid,
                publisher_user_id: post.created_by_user_principal_id.to_text(),
                post_id: post.id,
            })
        });

    Ok(items)
}
