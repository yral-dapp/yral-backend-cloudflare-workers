use candid::Principal;
use ic_agent::{identity::BasicIdentity, Agent};
use worker::{Env, Result};
use yral_canisters_client::individual_user_template::IndividualUserTemplate;
use yral_metadata_client::MetadataClient;

use crate::consts::AGENT_URL;

pub struct AdminCans {
    agent: Agent,
    metadata: MetadataClient<false>,
}

impl AdminCans {
    pub fn new(env: &Env) -> Result<Self> {
        let admin_pem = env.secret("BACKEND_ADMIN_IDENTITY")?.to_string();
        let id = BasicIdentity::from_pem(admin_pem.as_bytes())
            .map_err(|e| worker::Error::RustError(e.to_string()))?;

        let agent = Agent::builder()
            .with_url(AGENT_URL)
            .with_identity(id)
            .build()
            .map_err(|e| worker::Error::RustError(e.to_string()))?;

        Ok(Self {
            agent,
            metadata: MetadataClient::default(),
        })
    }

    pub async fn user_principal_to_user_canister(
        &self,
        user_principal: Principal,
    ) -> Result<Option<Principal>> {
        let user_meta = self
            .metadata
            .get_user_metadata(user_principal)
            .await
            .map_err(|e| worker::Error::RustError(e.to_string()))?;
        let Some(user_canister) = user_meta.map(|u| u.user_canister_id) else {
            return Ok(None);
        };

        // The lines below harden the security
        // as the worker makes multiple calls on behalf of the user
        // we need to ensure the user really owns this canister
        let user = self.individual_user(user_canister);
        let profile = user
            .get_profile_details_v_2()
            .await
            .map_err(|e| worker::Error::RustError(e.to_string()))?;
        if profile.principal_id != user_principal {
            return Ok(None);
        }

        Ok(Some(user_canister))
    }

    pub fn individual_user(&self, user_canister: Principal) -> IndividualUserTemplate<'_> {
        IndividualUserTemplate(user_canister, &self.agent)
    }
}
