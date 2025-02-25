use std::{collections::HashMap, error::Error, ops::Add};

use axum::http::{header, HeaderMap};
use ic_agent::export::reqwest;
use serde::{Deserialize, Serialize};
use worker::{Date, DateInit, Url};

use crate::utils::types::{DirectUploadRequestType, ResponseInfo, StreamResponseType};

use super::types::{DirectUploadResult, Video};

#[derive(Clone)]
pub struct CloudflareStream {
    client: reqwest::Client,
    base_url: Url,
}

impl CloudflareStream {
    pub fn new(account_id: String, api_token: String) -> Result<Self, Box<dyn Error>> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {api_token}").parse().unwrap(),
        );
        let client = reqwest::ClientBuilder::new()
            .default_headers(headers)
            .build()?;
        let base_url = Url::parse(&format!(
            "https://api.cloudflare.com/client/v4/accounts/{account_id}/stream/"
        ))?;

        Ok(Self { base_url, client })
    }

    pub async fn get_upload_url(&self) -> Result<DirectUploadResult, Box<dyn Error>> {
        type DirectUploadResponseType = StreamResponseType<DirectUploadResult>;
        let url = Url::join(&self.base_url, "direct_upload".into())?;

        let scheduled_deletion = Date::now().as_millis().add(1000 * 60 * 60); // 1hour
        let request_data = DirectUploadRequestType {
            scheduled_deletion: Some(Date::new(DateInit::Millis(scheduled_deletion)).to_string()),
            ..Default::default()
        };
        let response = self.client.post(url).json(&request_data).send().await?;
        let response_data: DirectUploadResponseType = response.json().await?;

        if response_data.success {
            let data = response_data.result.ok_or("Data not found")?;
            Ok(data)
        } else {
            let error = response_data.errors.get(0).ok_or("Unkown Error")?;

            Err(format!("Error: {} {}", error.code, error.message).into())
        }
    }

    pub async fn get_video_details(&self, video_uid: &str) -> Result<Video, Box<dyn Error>> {
        let url = Url::join(&self.base_url, &format!("/{video_uid}"))?;

        let response = self.client.get(url).send().await?;

        let response_data: StreamResponseType<Video> = response.json().await?;

        if response_data.success {
            response_data.result.ok_or("video details not found".into())
        } else {
            let error = response_data.errors.get(0).ok_or("Unknown error")?;
            Err(format!("{} {}", error.code, error.message).into())
        }
    }

    pub async fn add_meta_to_video(
        &self,
        video_uid: &str,
        meta: HashMap<String, String>,
    ) -> Result<(), Box<dyn Error>> {
        let url = Url::join(&self.base_url, &format!("/{video_uid}"))?;

        #[derive(Serialize, Deserialize)]
        struct EditVideoRequestType {
            meta: HashMap<String, String>,
            #[serde(rename = "scheduledDeletion")]
            scheduled_deletion: Option<String>,
        }

        #[derive(Serialize, Deserialize)]
        struct EditVideoResponseType {
            errors: Vec<ResponseInfo>,
            messages: Option<Vec<ResponseInfo>>,
            success: bool,
        }

        let response = self
            .client
            .post(url)
            .json(&EditVideoRequestType {
                meta,
                scheduled_deletion: None,
            })
            .send()
            .await?;

        let response_data: EditVideoResponseType = response.json().await?;

        if response_data.success {
            Ok(())
        } else {
            let error = response_data.errors.get(0).ok_or("Unknown error")?;
            Err(format!("{} {}", error.code, error.message).into())
        }
    }
}
