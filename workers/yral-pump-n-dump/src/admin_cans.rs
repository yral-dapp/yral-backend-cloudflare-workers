use candid::Principal;
use ic_agent::identity::{BasicIdentity, Secp256k1Identity};
use k256::SecretKey;
use worker::{Env, Result};
use yral_canisters_client::individual_user_template::IndividualUserTemplate;
use yral_metadata_client::MetadataClient;

use crate::{
    agent_wrapper::AgentWrapper,
    consts::{ADMIN_LOCAL_SECP_SK, LOCAL_METADATA_API_BASE},
    utils::{env_kind, RunEnv},
};

pub struct AdminCans {
    agent: AgentWrapper,
    metadata: MetadataClient<false>,
}

impl AdminCans {
    pub fn new(env: &Env) -> Result<Self> {
        let agent;
        let metadata;

        match env_kind() {
            RunEnv::Local => {
                let id = Secp256k1Identity::from_private_key(
                    SecretKey::from_bytes(&ADMIN_LOCAL_SECP_SK.into()).unwrap(),
                );
                agent = AgentWrapper::new(id);
                metadata = MetadataClient::with_base_url(LOCAL_METADATA_API_BASE.parse().unwrap());
            }
            RunEnv::Remote => {
                let admin_pem = env.secret("BACKEND_ADMIN_KEY")?.to_string();
                let id = BasicIdentity::from_pem(admin_pem.as_bytes())
                    .map_err(|e| worker::Error::RustError(e.to_string()))?;
                agent = AgentWrapper::new(id);
                metadata = MetadataClient::default();
            }
            RunEnv::Mock => panic!("trying to use ic-agent in mock env"),
        };

        Ok(Self { agent, metadata })
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
        let user = self.individual_user(user_canister).await;
        let profile = user
            .get_profile_details_v_2()
            .await
            .map_err(|e| worker::Error::RustError(e.to_string()))?;
        if profile.principal_id != user_principal {
            return Ok(None);
        }

        Ok(Some(user_canister))
    }

    pub async fn individual_user(&self, user_canister: Principal) -> IndividualUserTemplate<'_> {
        IndividualUserTemplate(user_canister, self.agent.get().await)
    }
}
