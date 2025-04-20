use jsonwebtoken::{get_current_timestamp, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub exp: usize,
}

fn main() {
    let enc_key_raw = fs::read("./pd_jwt.pem").expect("`./pd_jwt.pem` is required");
    let enc_key = EncodingKey::from_ed_pem(&enc_key_raw).expect("invalid `./pd_jwt.pem`");

    let header = Header::new(jsonwebtoken::Algorithm::EdDSA);
    // 180 days
    let expiry = get_current_timestamp() + (180 * 24 * 60 * 60);

    let claims = Claims {
        exp: expiry as usize,
    };

    let token = jsonwebtoken::encode(&header, &claims, &enc_key).expect("failed to encode JWT");
    println!("JWT with 180 days expiry:");
    println!("{token}");
}
