use jsonwebtoken::DecodingKey;
use serde::{Deserialize, Serialize};

const JWT_PUBKEY: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAV+DJfztWOovpmCUcZ5Fram2BLOt2B4LIlzw2vogIqK4=
-----END PUBLIC KEY-----";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub exp: usize,
}

pub fn verify_jwt(jwt: &str) -> Result<(), jsonwebtoken::errors::Error> {
    jsonwebtoken::decode::<Claims>(
        jwt,
        &DecodingKey::from_ed_pem(JWT_PUBKEY.as_bytes()).unwrap(),
        &Default::default(),
    )?;

    Ok(())
}
