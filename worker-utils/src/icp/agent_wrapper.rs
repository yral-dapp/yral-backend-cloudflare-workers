use crate::environment::{RunEnv, env_kind};
use candid::Principal;
use ic_agent::{Agent, AgentError, Identity};

pub const fn agent_url() -> &'static str {
    match env_kind() {
        RunEnv::Remote => "https://ic0.app",
        RunEnv::Local => "http://localhost:4943",
        RunEnv::Mock => panic!("trying to get `AGENT_URL` in mock env"),
    }
}

#[derive(Clone)]
pub struct AgentWrapper(Agent);

impl AgentWrapper {
    pub fn new(id: impl Identity + 'static) -> Self {
        let agent = Agent::builder()
            .with_url(agent_url())
            .with_identity(id)
            .build()
            .unwrap();
        Self(agent)
    }

    pub async fn get(&self) -> &Agent {
        let agent = &self.0;
        match env_kind() {
            RunEnv::Local => agent
                .fetch_root_key()
                .await
                .expect("AGENT: fetch_root_key failed"),
            RunEnv::Mock => {
                panic!("Calling ic-agent from mock env?!");
            }
            RunEnv::Remote => (),
        };
        agent
    }

    pub async fn canister_controller(&self, canister: Principal) -> Result<Principal, AgentError> {
        let res = self
            .0
            .read_state_canister_info(canister, "controllers")
            .await?;
        let controllers: Vec<Principal> =
            ciborium::from_reader(res.as_slice()).expect("ic0 returned invalid controllers?!");
        Ok(controllers[0])
    }
}
