use std::{collections::HashMap, error::Error};

use ic_agent::identity::{DelegatedIdentity, Secp256k1Identity, SignedDelegation};
use k256::elliptic_curve::JwkEcKey;
use serde::{Deserialize, Serialize};

pub const DELEGATED_IDENTITY_KEY: &'static str = "delegated-identity";
pub const POST_DETAILS_KEY: &'static str = "post-details";

#[derive(Serialize, Deserialize, Clone)]
pub struct NotifyStatusType {
    pub state: String,
    pub step: Option<String>,
    #[serde(rename = "pctComplete")]
    pub pct_complete: Option<String>,
    #[serde(rename = "errReasonCode")]
    pub err_reason_code: Option<String>,
    #[serde(rename = "errReasonText")]
    pub err_reason_text: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NotifyRequestPayload {
    pub uid: String,
    #[serde(rename = "readyToStream")]
    pub ready_to_stream: bool,
    pub status: NotifyStatusType,
    pub meta: HashMap<String, String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Video {
    #[serde(rename = "allowedOrigins", skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>, // Array<AllowedOrigins>

    #[serde(rename = "created", skip_serializing_if = "Option::is_none")]
    pub created: Option<String>, // format: date-time

    #[serde(rename = "creator", skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>, // maxLength: 64

    #[serde(rename = "duration", skip_serializing_if = "Option::is_none")]
    pub duration: Option<i32>, // number, -1 means unknown

    #[serde(rename = "input", skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>, // { height, width }

    #[serde(rename = "liveInput", skip_serializing_if = "Option::is_none")]
    pub live_input: Option<String>, // maxLength: 32

    #[serde(rename = "maxDurationSeconds", skip_serializing_if = "Option::is_none")]
    pub max_duration_seconds: Option<i32>, // maximum: 21600, minimum: 1, -1 means unknown

    #[serde(rename = "meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, String>>, // unknown, key-value store

    #[serde(rename = "modified", skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>, // format: date-time

    #[serde(rename = "playback", skip_serializing_if = "Option::is_none")]
    pub playback: Option<Playback>, // { dash, hls }

    #[serde(rename = "preview", skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>, // format: uri

    #[serde(rename = "readyToStream", skip_serializing_if = "Option::is_none")]
    pub ready_to_stream: Option<bool>, // boolean

    #[serde(rename = "readyToStreamAt", skip_serializing_if = "Option::is_none")]
    pub ready_to_stream_at: Option<String>, // format: date-time

    #[serde(rename = "requireSignedURLs", skip_serializing_if = "Option::is_none")]
    pub require_signed_urls: Option<bool>, // boolean

    #[serde(rename = "scheduledDeletion", skip_serializing_if = "Option::is_none")]
    pub scheduled_deletion: Option<String>, // format: date-time

    #[serde(rename = "size", skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>, // number (size in bytes)

    #[serde(rename = "status", skip_serializing_if = "Option::is_none")]
    pub status: Option<NotifyStatusType>, // { errorReasonCode, errorReasonText, pctComplete, 1 more... }

    #[serde(rename = "thumbnail", skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>, // format: uri

    #[serde(
        rename = "thumbnailTimestampPct",
        skip_serializing_if = "Option::is_none"
    )]
    pub thumbnail_timestamp_pct: Option<f32>, // maximum: 1, minimum: 0

    #[serde(rename = "uid", skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>, // maxLength: 32

    #[serde(rename = "uploaded", skip_serializing_if = "Option::is_none")]
    pub uploaded: Option<String>, // format: date-time

    #[serde(rename = "uploadExpiry", skip_serializing_if = "Option::is_none")]
    pub upload_expiry: Option<String>, // format: date-time

    #[serde(rename = "watermark", skip_serializing_if = "Option::is_none")]
    pub watermark: Option<Watermark>, // Watermark
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Input {
    #[serde(rename = "height")]
    pub height: Option<u32>,

    #[serde(rename = "width")]
    pub width: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Playback {
    #[serde(rename = "dash")]
    pub dash: Option<String>,

    #[serde(rename = "hls")]
    pub hls: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ResponseInfo {
    pub code: u32,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Watermark {
    created: Option<String>,
    #[serde(rename = "downloadFrom")]
    download_from: Option<String>,
    height: Option<f32>,
    name: Option<String>,
    opacity: Option<f32>,
    padding: Option<String>,
    scale: Option<f32>,
    uid: Option<String>,
    width: Option<f32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DirectUploadResult {
    #[serde(rename = "scheduledDeletion")]
    pub scheduled_deletion: Option<String>,
    pub uid: Option<String>,
    #[serde(rename = "uploadUrl")]
    pub upload_url: Option<String>,
    pub watermark: Option<Watermark>,
}

#[derive(Serialize, Deserialize)]
pub struct StreamResponseType<T> {
    pub errors: Vec<ResponseInfo>,
    pub messages: Option<Vec<ResponseInfo>>,
    pub success: bool,
    pub result: Option<T>,
}

/// Delegated identity that can be serialized over the wire
#[derive(Serialize, Deserialize, Clone)]
pub struct DelegatedIdentityWire {
    /// raw bytes of delegated identity's public key
    pub from_key: Vec<u8>,
    /// JWK(JSON Web Key) encoded Secp256k1 secret key
    /// identity allowed to sign on behalf of `from_key`
    pub to_secret: JwkEcKey,
    /// Proof of delegation
    /// connecting from_key to `to_secret`
    pub delegation_chain: Vec<SignedDelegation>,
}

impl std::fmt::Debug for DelegatedIdentityWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegatedIdentityWire").finish()
    }
}

impl TryFrom<DelegatedIdentityWire> for DelegatedIdentity {
    type Error = Box<dyn Error>;

    fn try_from(value: DelegatedIdentityWire) -> Result<DelegatedIdentity, Box<dyn Error>> {
        let to_secret = k256::SecretKey::from_jwk(&value.to_secret)?;
        let to_identity = Secp256k1Identity::from_private_key(to_secret);
        Ok(Self::new(
            value.from_key,
            Box::new(to_identity),
            value.delegation_chain,
        ))
    }
}
