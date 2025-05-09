use jsonwebtoken::{EncodingKey, Header, get_current_timestamp};
use std::{env, fs};
use yral_worker_utils::jwt;

fn main() {
    let jwt_pem_file = env::var("JWT_PEM_FILE").expect("JWT_PEM_FILE is required");
    let jwt_aud = env::var("JWT_AUD").expect("JWT_AUD is required");
    let enc_key_raw = fs::read(jwt_pem_file).expect("JWT_PEM_FILE is not valid");
    let enc_key = EncodingKey::from_ed_pem(&enc_key_raw).expect("invalid JWT_PEM_FILE");

    let header = Header::new(jsonwebtoken::Algorithm::EdDSA);
    // 180 days
    let expiry = get_current_timestamp() + (180 * 24 * 60 * 60);

    let claims = jwt::Claims {
        aud: jwt_aud,
        exp: expiry as usize,
    };

    let token = jsonwebtoken::encode(&header, &claims, &enc_key).expect("failed to encode JWT");

    println!("JWT with 180 days expiry:");
    println!("{token}");
}
