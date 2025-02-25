use candid::Principal;
use serde_json::json;
use std::error::Error;
use tonic_web_wasm_client::Client;
use warehouse_event::{warehouse_events_client::WarehouseEventsClient, WarehouseEvent};

pub mod warehouse_event {
    include!(concat!(env!("OUT_DIR"), "/warehouse_events.rs"));
}

#[derive(Clone)]
pub struct Warehouse {
    pub client: WarehouseEventsClient<Client>,
}

impl Default for Warehouse {
    fn default() -> Self {
        let client = Client::new("https://offchain-agent.fly.dev".to_string());

        Self {
            client: WarehouseEventsClient::new(client),
        }
    }
}

impl Warehouse {
    pub async fn send_video_upload_successful_event(
        &mut self,
        video_uid: String,
        hashtags_len: usize,
        is_nsfw: bool,
        enable_hot_or_not: bool,
        post_id: u64,
        user_principal: Principal,
        canister_id: Principal,
        user_name: String,
    ) -> Result<(), Box<dyn Error>> {
        let params = json!({
            "user_id": user_principal,
            "publisher_user_id": user_principal,
            "display_name": user_name,
            "canister_id": canister_id,
            "creator_category": "NA",
            "hashtag_count": hashtags_len,
            "is_NSFW": is_nsfw,
            "is_hotorNot": enable_hot_or_not,
            "is_filter_used": false,
            "video_id": video_uid,
            "post_id": post_id,
        })
        .to_string();

        let request = tonic::Request::new(warehouse_event::WarehouseEvent {
            event: "video_upload_successful".to_string(),
            params,
        });

        self.client.send_event(request).await?;

        Ok(())
    }

    pub async fn send_video_event_unsuccessful(
        &mut self,
        error: String,
        hashtags_len: usize,
        is_nsfw: bool,
        enable_hot_or_not: bool,
        user_principal: Principal,
        user_name: String,
        user_canister: Principal,
    ) -> Result<(), Box<dyn Error>> {
        let params = json!({
            "user_id": user_principal,
            "display_name": user_name,
            "canister_id": user_canister,
            "creator_category": "NA",
            "hashtag_count": hashtags_len,
            "is_NSFW": is_nsfw,
            "is_hotorNot": enable_hot_or_not,
            "fail_reason": error,
        })
        .to_string();

        let request = tonic::Request::new(warehouse_event::WarehouseEvent {
            event: "video_upload_unsuccessful".to_string(),
            params,
        });

        self.client.send_event(request).await?;

        Ok(())
    }
}
