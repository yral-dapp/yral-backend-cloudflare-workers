use std::error::Error;

use ic_agent::{identity::DelegatedIdentity, Agent};
use worker::{console_log, console_warn};

use crate::utils::{
    events::{EventService, Warehouse},
    individual_user_canister::{
        PostDetailsFromFrontend, Result2, Service as IndividualUserCanisterService,
    },
    types::{DelegatedIdentityWire, NotifyRequestPayload},
    user_ic_agent,
};

pub async fn upload_video_to_canister(
    mut events: EventService,
    video_uid: String,
    delegated_identity_wire: DelegatedIdentityWire,
    post_details: PostDetailsFromFrontend,
) -> Result<(), Box<dyn Error>> {
    let delegated_identity = DelegatedIdentity::try_from(delegated_identity_wire)?;
    let ic_agent = Agent::builder()
        .with_identity(delegated_identity)
        .with_url("https://ic0.app")
        .build()?;
    let yral_metadata_client = yral_metadata_client::MetadataClient::default();

    console_log!("user principal id {}", ic_agent.get_principal()?);

    let user_details = yral_metadata_client
        .get_user_metadata(ic_agent.get_principal()?)
        .await?
        .ok_or::<Box<dyn Error>>("user canister not found".into())?;

    console_log!("user canister id {}", user_details.user_canister_id);

    let individual_user_service =
        IndividualUserCanisterService(user_details.user_canister_id, &ic_agent);

    match upload_video_to_canister_inner(&individual_user_service, post_details.clone()).await {
        Ok(post_id) => {
            console_log!("video upload to canister successful");

            events
                .send_video_upload_successful_event(
                    video_uid,
                    post_details.hashtags.len(),
                    post_details.is_nsfw,
                    post_details.creator_consent_for_inclusion_in_hot_or_not,
                    post_id,
                    ic_agent.get_principal()?,
                    user_details.user_canister_id,
                    user_details.user_name,
                )
                .await
        }
        Err(e) => {
            console_warn!("video upload to canister unsuccessful");
            events
                .send_video_event_unsuccessful(
                    e.to_string(),
                    post_details.hashtags.len(),
                    post_details.is_nsfw,
                    post_details.creator_consent_for_inclusion_in_hot_or_not,
                    ic_agent.get_principal()?,
                    user_details.user_name,
                    user_details.user_canister_id,
                )
                .await
        }
    }
}

async fn upload_video_to_canister_inner(
    individual_user_canister: &IndividualUserCanisterService<'_>,
    post_details: PostDetailsFromFrontend,
) -> Result<u64, Box<dyn Error>> {
    let result = individual_user_canister.add_post_v_2(post_details).await?;
    match result {
        Result2::Ok(post_id) => Ok(post_id),
        Result2::Err(err) => Err(err.into()),
    }
}
