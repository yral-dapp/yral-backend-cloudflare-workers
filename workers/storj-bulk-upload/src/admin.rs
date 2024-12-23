use cfg_if::cfg_if;
use ic_agent::{export::Principal, identity::Secp256k1Identity, Identity};
use k256::SecretKey;
use worker::console_log;
use yral_canisters_client::{
    self, individual_user_template::IndividualUserTemplate,
    platform_orchestrator::PlatformOrchestrator, user_index::UserIndex,
};
use yral_canisters_common::agent_wrapper::AgentWrapper;

#[derive(Clone)]
pub struct AdminCanisters {
    agent: AgentWrapper,
}

impl AdminCanisters {
    pub fn new(key: impl Identity + 'static) -> Self {
        Self {
            agent: AgentWrapper::build(|b| b.with_identity(key)),
        }
    }

    cfg_if! {
        if #[cfg(feature = "local")] {
            pub fn get_identity() -> impl Identity + 'static {
                const ADMIN_SECP_BYTES: [u8; 32] = [
                    9, 64, 7, 55, 201, 208, 139, 219, 167, 201, 176, 6, 31, 109, 44, 248, 27, 241, 239, 56, 98,
                    100, 158, 36, 79, 233, 172, 151, 228, 187, 8, 224,
                ];
                let sk = SecretKey::from_bytes(&ADMIN_SECP_BYTES.into()).unwrap();
                Secp256k1Identity::from_private_key(sk)
            }
        } else {
            pub fn get_identity() -> impl Identity + 'static {
                todo!("actually load the secret key for admin");
                let bytes = [0; 32];
                let sk = SecretKey::from_bytes(&bytes.into()).unwrap();
                Secp256k1Identity::from_private_key(sk)
            }
        }
    }

    pub async fn platform_orchestrator(&self) -> PlatformOrchestrator<'_> {
        // Logically, local id should be used when running in local mode but the
        // backend returns `canister_not_found` when I actually use the local
        // version. oddly, when I use ic id, everything works locally?
        cfg_if! {
            if #[cfg(feature = "local")] {
                use yral_canisters_client::ic::PLATFORM_ORCHESTRATOR_ID;
            } else {
                use yral_canisters_client::local::PLATFORM_ORCHESTRATOR_ID;
            }
        }

        PlatformOrchestrator(PLATFORM_ORCHESTRATOR_ID, self.get_agent().await)
    }

    pub fn principal(&self) -> Principal {
        self.agent.principal().unwrap()
    }

    #[inline]
    async fn get_agent(&self) -> &ic_agent::Agent {
        self.agent.get_agent().await
    }

    pub async fn user_index_with(&self, idx_principal: Principal) -> UserIndex<'_> {
        UserIndex(idx_principal, self.get_agent().await)
    }

    pub async fn individual_user_for(
        &self,
        user_canister: Principal,
    ) -> IndividualUserTemplate<'_> {
        IndividualUserTemplate(user_canister, self.get_agent().await)
    }
}
