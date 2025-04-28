use candid::Principal;
use serde::{Deserialize, Serialize};
use worker::{console_error, console_log};

const METADATA_SERVER_URL: &str = "https://yral-metadata.fly.dev/";

#[derive(Serialize, Deserialize)]
pub enum NotificationType {
    VideoUploadSuccess(String),
    VideoUploadError(String),
    VideoProcessingError(String),
    VideoStatusExtractionError(String),
}

pub async fn send_notification(
    data: NotificationType,
    creator: Option<Principal>,
    video_uid: &str,
    notif_api_key: String,
) {
    if let Some(creator_principal) = creator {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/notifications/{}/send",
            METADATA_SERVER_URL,
            creator_principal.to_text()
        );

        let res = client
            .post(&url)
            .bearer_auth(notif_api_key)
            .json(&data)
            .send()
            .await;

        match res {
            Ok(response) => {
                if response.status().is_success() {
                    console_log!(
                        "Successfully sent error notification for video {}",
                        video_uid
                    );
                } else {
                    console_error!(
                        "Failed to send error notification for video {}: Status: {}",
                        video_uid,
                        response.status()
                    );
                    if let Ok(body) = response.text().await {
                        console_error!("Response body: {}", body);
                    }
                }
            }
            Err(req_err) => {
                console_error!(
                    "Error sending notification request for video {}: {}",
                    video_uid,
                    req_err
                );
            }
        }
    } else {
        console_error!(
            "Creator principal not found for video {}, cannot send notification.",
            video_uid
        );
    }
}
