use std::collections::HashSet;

use jsonwebtoken::DecodingKey;
use serde::{Deserialize, Serialize};

const JWT_PUBKEY: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAV+DJfztWOovpmCUcZ5Fram2BLOt2B4LIlzw2vogIqK4=
-----END PUBLIC KEY-----";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub aud: String,
    pub exp: usize,
}

pub fn verify_jwt(jwt: &str) -> Result<(), jsonwebtoken::errors::Error> {
    let mut validation = jsonwebtoken::Validation::default();
    validation.aud = Some(HashSet::from([String::from("pump-n-dump-worker")]));
    validation.algorithms = vec![jsonwebtoken::Algorithm::EdDSA];

    jsonwebtoken::decode::<Claims>(
        jwt,
        &DecodingKey::from_ed_pem(JWT_PUBKEY.as_bytes()).unwrap(),
        &validation,
    )?;

    Ok(())
}
