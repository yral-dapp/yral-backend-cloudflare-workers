use std::error::Error;

use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::console_log;

use crate::{
    utils::{
        events::EventService,
        individual_user_canister::PostDetailsFromFrontend,
        types::{NotifyRequestPayload, DELEGATED_IDENTITY_KEY, POST_DETAILS_KEY},
    },
    DelegatedIdentityWire,
};

use super::upload_video_to_canister::upload_video_to_canister;

pub fn verify_webhook_signature(
    webhook_secret_key: String,
    webhook_signature: &str,
    req_data: String,
) -> Result<(), Box<dyn Error>> {
    let mut time_and_signature = webhook_signature.split(",");

    let time = time_and_signature
        .next()
        .ok_or("time not found in web signature")?
        .split("=")
        .last()
        .ok_or("invalid time header format")?;

    let signature = time_and_signature
        .next()
        .ok_or("signature not found in web signature")?
        .split("=")
        .last()
        .ok_or("invalid signature header format")?;

    let input_str = format!("{time}.{req_data}");

    type HmacSha256 = Hmac<Sha256>;

    let mut hmac = HmacSha256::new_from_slice(webhook_secret_key.as_bytes())?;

    hmac.update(input_str.as_bytes());

    let mac_result = hmac.finalize();
    let result_str = mac_result.into_bytes();
    let digest = hex::encode(result_str);

    if digest.eq(&signature) {
        Ok(())
    } else {
        Err("Invalid webhook signature".into())
    }
}
pub async fn notify_video_upload_impl(
    events: EventService,
    req_data: String,
    headers: HeaderMap,
    webhook_secret_key: String,
) -> Result<(), Box<dyn Error>> {
    let webhook_signature = headers
        .get("Webhook-Signature")
        .ok_or("Signature not found")?
        .to_str()?;

    let notify_req_paylod: NotifyRequestPayload = serde_json::from_str(&req_data)?;

    verify_webhook_signature(webhook_secret_key, webhook_signature, req_data)?;

    if notify_req_paylod
        .status
        .state
        .is_some_and(|state| state.eq("error"))
    {
        return Err(notify_req_paylod
            .status
            .err_reason_text
            .unwrap_or("unknown error while processing video".into())
            .into());
    }

    let Some(delegated_identity_string) = notify_req_paylod.meta.get(DELEGATED_IDENTITY_KEY) else {
        console_log!("Delegated identity not found in meta");
        return Err("Delegated identity metadata not found".into());
    };

    let delegated_identity_wire: DelegatedIdentityWire =
        serde_json::from_str(delegated_identity_string)?;

    let post_details_string = notify_req_paylod
        .meta
        .get(POST_DETAILS_KEY)
        .ok_or("post details not found")?;

    let post_details: PostDetailsFromFrontend = serde_json::from_str(post_details_string)?;

    upload_video_to_canister(
        &events,
        notify_req_paylod.uid,
        delegated_identity_wire,
        post_details,
    )
    .await
}
