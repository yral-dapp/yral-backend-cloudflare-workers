use candid::Principal;

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
pub const LOCAL_METADATA_API_BASE: &str = "http://localhost:8001";
// [0, 0, 0, 0, 2, 0, 0, 43, 1, 1]
pub const DOLR_LEDGER: Principal = Principal::from_slice(&[0, 0, 0, 0, 2, 0, 0, 43, 1, 1]);
// 100 DOLLR
pub const MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER: u64 = 100 * 1e8 as u64;
// 400 DOLLR
pub const USER_INDEX_FUND_AMOUNT: u64 = 400 * 1e8 as u64;
