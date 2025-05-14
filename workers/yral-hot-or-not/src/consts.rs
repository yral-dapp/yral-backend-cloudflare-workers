use candid::Principal;

pub const DEFAULT_ONBOARDING_REWARD_SATS: u64 = 1000;
// mxzaz-hqaaa-aaaar-qaada-cai
pub const CKBTC_LEDGER: Principal = Principal::from_slice(&[0, 0, 0, 0, 2, 48, 0, 6, 1, 1]);
// 1000 Satoshis
pub const MAXIMUM_CKBTC_TREASURY_PER_DAY_PER_USER: u32 = 10000u32;
