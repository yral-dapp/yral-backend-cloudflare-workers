use std::error::Error;

use ic_agent::Agent;

use super::individual_user_canister::Service as UserCanisterService;
use yral_metadata_client::MetadataClient;

#[derive(Clone)]
pub struct UserICAgent<'a> {
    // need to find the canister id as well.
    pub user_canister_service: UserCanisterService<'a>,
}

impl<'a> UserICAgent<'a> {
    async fn new(ic_agent: &'a Agent) -> Result<Self, Box<dyn Error>> {
        let user_principal = ic_agent.get_principal()?;

        let yral_metadata_client = MetadataClient::default();

        let user_metadata = yral_metadata_client
            .get_user_metadata(user_principal)
            .await?
            .ok_or("user canister not found")?;

        Ok(Self {
            user_canister_service: UserCanisterService(user_metadata.user_canister_id, &ic_agent),
        })
    }
}
