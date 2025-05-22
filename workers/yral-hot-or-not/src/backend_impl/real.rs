use candid::Principal;
use worker::Result;
use yral_canisters_client::individual_user_template::{Result9, SessionType};

use crate::admin_cans::AdminCans;

use super::UserStateBackendImpl;

impl UserStateBackendImpl for AdminCans {
    async fn is_user_registered(&self, user_canister: Principal) -> Result<bool> {
        let user = self.individual_user(user_canister).await;
        let res = user
            .get_session_type()
            .await
            .map_err(|e| worker::Error::RustError(e.to_string()))?;

        Ok(res == Result9::Ok(SessionType::RegisteredSession))
    }
}
