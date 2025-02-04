use ic_agent::{Agent, Identity};

use crate::{
    consts::agent_url,
    utils::{env_kind, RunEnv},
};

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
}
