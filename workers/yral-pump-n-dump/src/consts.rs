use crate::utils::{env_kind, RunEnv};

pub const GDOLLR_TO_DOLLR: u64 = 100;
pub const DOLLR_TO_E8S: u64 = 1e8 as u64;
pub const GDOLLR_TO_E8S: u64 = DOLLR_TO_E8S / GDOLLR_TO_DOLLR;
pub const TIDE_SHIFT_DELTA: u64 = 1;
/// sync user state after 60 seconds
pub const USER_STATE_RECONCILE_TIME_MS: i64 = 60 * 1000;
pub const ADMIN_LOCAL_SECP_SK: [u8; 32] = [
    9, 64, 7, 55, 201, 208, 139, 219, 167, 201, 176, 6, 31, 109, 44, 248, 27, 241, 239, 56, 98,
    100, 158, 36, 79, 233, 172, 151, 228, 187, 8, 224,
];

pub const fn agent_url() -> &'static str {
    match env_kind() {
        RunEnv::Remote => "https://ic0.app",
        RunEnv::Local => "http://localhost:4943",
        RunEnv::Mock => panic!("trying to get `AGENT_URL` in mock env"),
    }
}
