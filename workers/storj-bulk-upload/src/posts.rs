use anyhow::Context;
use futures::{future, stream, StreamExt, TryStreamExt};

use crate::admin::AdminCanisters;

#[derive(Debug, serde::Deserialize, serde::Serialize, Default)]
pub(crate) struct Item {
    video_id: String,
    publisher_user_id: String,
    // TODO: extra metadata
}

pub(crate) async fn load_items(
    admin: &AdminCanisters,
) -> anyhow::Result<impl futures::Stream<Item = Item>> {
    let subs = admin
        .platform_orchestrator()
        .await
        .get_all_subnet_orchestrators()
        .await
        .context("Couldn't fetch the subnet orchestrator")?;

    let mut users = stream::iter(subs)
        .then(|sub| async move { admin.user_index_with(sub).await })
        .then(|index| async move { index.get_user_canister_list().await })
        .and_then(|list| future::ok(stream::iter(list).map(anyhow::Ok)))
        .try_flatten()
        .and_then(|user_principal| async move {
            anyhow::Ok(admin.individual_user_for(user_principal).await)
        });

    // TODO: iterate over each user and grab posts using the paginated getter

    Ok(stream::once(async { Item::default() }))
}
