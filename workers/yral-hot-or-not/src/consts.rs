use candid::Principal;

pub const DEFAULT_ONBOARDING_REWARD_SATS: u64 = 1000;
// mxzaz-hqaaa-aaaar-qaada-cai
pub const CKBTC_LEDGER: Principal = Principal::from_slice(&[0, 0, 0, 0, 2, 48, 0, 6, 1, 1]);
// 1000 Satoshis
pub const MAXIMUM_CKBTC_TREASURY_PER_DAY_PER_USER: u32 = 10000u32;
pub const MAXIMUM_VOTE_AMOUNT_SATS: u128 = 200;

pub const REFERRAL_REWARD_REFERRER_SATS: u64 = 500;
pub const REFERRAL_REWARD_REFEREE_SATS: u64 = 500;

pub const ADMIN_LOCAL_SECP_SK: [u8; 32] = [
    9, 64, 7, 55, 201, 208, 139, 219, 167, 201, 176, 6, 31, 109, 44, 248, 27, 241, 239, 56, 98,
    100, 158, 36, 79, 233, 172, 151, 228, 187, 8, 224,
];
pub const LOCAL_METADATA_API_BASE: &str = "http://localhost:8001";
