use server_impl::notify_video_upload_impl::notify_video_upload_impl;
use server_impl::upload_video_to_canister::upload_video_to_canister;
use std::collections::HashMap;
use std::fmt::Display;
use std::result::Result;
use std::{error::Error, sync::Arc};
use utils::individual_user_canister::PostDetailsFromFrontend;
use utils::types::{
    DelegatedIdentityWire, DirectUploadResult, NotifyRequestPayload, DELEGATED_IDENTITY_KEY,
    POST_DETAILS_KEY,
};

use axum::http::HeaderMap;
use axum::{
    debug_handler,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_service::Service;
use utils::cloudflare_stream::CloudflareStream;
use utils::events::Warehouse;
use worker::Result as WorkerResult;
use worker::*;

use axum::extract::State;

pub mod server_impl;
pub mod utils;

#[derive(Serialize, Deserialize, Debug)]
pub struct APIResponse<T> {
    pub message: Option<String>,
    pub success: bool,
    pub data: Option<T>,
}

#[derive(Serialize, Deserialize)]
pub struct VideoKvStoreValue {
    pub user_delegated_identity_wire: Option<DelegatedIdentityWire>,
    pub meta: Option<HashMap<String, String>>,
    pub direct_upload_result: DirectUploadResult,
}

impl<T, E> From<Result<T, E>> for APIResponse<T>
where
    E: Display,
{
    fn from(value: Result<T, E>) -> Self {
        match value {
            Ok(data) => Self {
                message: None,
                success: true,
                data: Some(data),
            },
            Err(err) => Self {
                message: Some(format!("{}", err.to_string())),
                success: false,
                data: None,
            },
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub cloudflare_stream: CloudflareStream,
    pub events: Warehouse,
    pub webhook_secret_key: String,
}

impl AppState {
    fn new(
        clouflare_account_id: String,
        cloudflare_api_token: String,
        webhook_secret_key: String,
    ) -> Result<Self, Box<dyn Error>> {
        let cloudflare_stream = CloudflareStream::new(clouflare_account_id, cloudflare_api_token)?;
        Ok(Self {
            cloudflare_stream,
            events: Warehouse::default(),
            webhook_secret_key,
        })
    }
}

fn router(env: Env, ctx: Context) -> Router {
    let app_state = AppState::new(
        env.secret("CLOUDFLARE_STREAM_ACCOUNT_ID")
            .unwrap()
            .to_string(),
        env.secret("CLOUDFLARE_STREAM_API_TOKEN")
            .unwrap()
            .to_string(),
        env.secret("CLOUDFLARE_STREAM_WEBHOOK_SECRET")
            .unwrap()
            .to_string(),
    )
    .unwrap();

    Router::new()
        .route("/", get(root))
        .route("/get_upload_url", get(get_upload_url))
        .route("/update_metadata", post(update_metadata))
        .route("/notify", post(notify_video_upload))
        .with_state(Arc::new(app_state))
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    ctx: Context,
) -> WorkerResult<axum::http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();
    Ok(router(env, ctx).call(req).await?)
}

pub async fn root() -> &'static str {
    "Hello Axum!"
}

#[derive(Serialize, Deserialize)]
struct UpdateMetadataRequest {
    video_uid: String,
    delegated_identity_wire: DelegatedIdentityWire,
    meta: HashMap<String, String>,
    post_details: PostDetailsFromFrontend,
}

#[debug_handler]
#[worker::send]
pub async fn update_metadata(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateMetadataRequest>,
) -> Json<APIResponse<()>> {
    Json(APIResponse::from(
        update_metadata_impl(&app_state.cloudflare_stream, payload).await,
    ))
}

#[debug_handler]
#[worker::send]
pub async fn notify_video_upload(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<NotifyRequestPayload>,
) -> Json<APIResponse<()>> {
    console_log!("Notify Recieved: {:?}", &payload);
    let result =
        notify_video_upload_impl(payload, headers, app_state.webhook_secret_key.clone()).await;

    match result {
        Ok(()) => Json(APIResponse::from(Ok::<(), Box<dyn Error>>(()))),
        Err(e) => {
            console_error!("Error processing Notify: {}", e.to_string());
            Json(APIResponse::from(Err(e)))
        }
    }
}

async fn update_metadata_impl(
    cloudflare_stream: &CloudflareStream,
    mut req_data: UpdateMetadataRequest,
) -> Result<(), Box<dyn Error>> {
    let video_details = cloudflare_stream
        .get_video_details(&req_data.video_uid)
        .await?;

    if let Some(ready_to_stream) = video_details.ready_to_stream {
        if ready_to_stream {
            return upload_video_to_canister(
                req_data.video_uid,
                req_data.delegated_identity_wire,
                req_data.post_details,
            )
            .await;
        }
    }

    req_data.meta.insert(
        DELEGATED_IDENTITY_KEY.to_string(),
        serde_json::to_string(&req_data.delegated_identity_wire)?,
    );

    req_data.meta.insert(
        POST_DETAILS_KEY.to_string(),
        serde_json::to_string(&req_data.post_details)?,
    );

    cloudflare_stream
        .add_meta_to_video(&req_data.video_uid, req_data.meta)
        .await?;

    Ok(())
}

#[debug_handler]
#[worker::send]
pub async fn get_upload_url(
    State(app_state): State<Arc<AppState>>,
) -> Json<APIResponse<DirectUploadResult>> {
    Json(APIResponse::from(
        get_upload_url_impl(&app_state.cloudflare_stream).await,
    ))
}

async fn get_upload_url_impl(
    cloudflare_stream: &CloudflareStream,
) -> Result<DirectUploadResult, Box<dyn Error>> {
    let result = cloudflare_stream.get_upload_url().await?;
    Ok(result)
}
