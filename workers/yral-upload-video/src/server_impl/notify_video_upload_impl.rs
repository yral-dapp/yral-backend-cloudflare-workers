use std::error::Error;

use hmac::{Hmac, Mac};
use sha2::Sha256;

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
