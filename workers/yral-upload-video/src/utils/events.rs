use candid::Principal;
use serde_json::json;
use std::error::Error;
use tonic::metadata::MetadataValue;
use tonic_web_wasm_client::{options::FetchOptions, Client};
use warehouse_event::{warehouse_events_client::WarehouseEventsClient, WarehouseEvent};

pub mod warehouse_event {
    include!(concat!(env!("OUT_DIR"), "/warehouse_events.rs"));
}

#[derive(Clone)]
pub struct Warehouse {
    pub client: WarehouseEventsClient<Client>,
    off_chain_agent_grpc_auth_token: String,
}

impl Warehouse {
    pub fn with_auth_token(auth_token: String) -> Self {
        let client = Client::new("https://icp-off-chain-agent.fly.dev:443".to_string());

        Self {
            client: WarehouseEventsClient::new(client),
            off_chain_agent_grpc_auth_token: auth_token,
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

        let mut request = tonic::Request::new(warehouse_event::WarehouseEvent {
            event: "video_upload_successful".to_string(),
            params,
        });

        let token: MetadataValue<_> =
            format!("Bearer {}", self.off_chain_agent_grpc_auth_token).parse()?;

        request
            .metadata_mut()
            .insert("authorization", token.clone());

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

        let mut request = tonic::Request::new(warehouse_event::WarehouseEvent {
            event: "video_upload_unsuccessful".to_string(),
            params,
        });

        let token: MetadataValue<_> =
            format!("Bearer {}", self.off_chain_agent_grpc_auth_token).parse()?;

        request
            .metadata_mut()
            .insert("authorization", token.clone());

        self.client.send_event(request).await?;

        Ok(())
    }
}
