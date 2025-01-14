pub const GDOLLR_TO_DOLLR: u64 = 100;
pub const DOLLR_TO_E8S: u64 = 1e8 as u64;
pub const GDOLLR_TO_E8S: u64 = DOLLR_TO_E8S / GDOLLR_TO_DOLLR;
pub const AGENT_URL: &str = "https://ic0.app";
pub const TIDE_SHIFT_DELTA: u64 = 10;
/// sync user state after 60 seconds
pub const USER_STATE_RECONCILE_TIME_MS: i64 = 60 * 1000;
