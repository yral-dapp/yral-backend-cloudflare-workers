use candid::Nat;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use worker::{Date, Result};
use worker_utils::storage::SafeStorage;

use crate::consts::MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER;

#[derive(Serialize, Deserialize, Clone)]
struct DolrTreasuryInner {
    amount: BigUint,
    last_reset_epoch: u64,
}

impl Default for DolrTreasuryInner {
    fn default() -> Self {
        Self {
            amount: BigUint::from(MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER),
            last_reset_epoch: Date::now().as_millis(),
        }
    }
}

#[derive(Default, Clone)]
pub struct DolrTreasury(Option<DolrTreasuryInner>);

impl DolrTreasury {
    async fn get_or_init(&mut self, storage: &SafeStorage) -> Result<&mut DolrTreasuryInner> {
        if self.0.is_some() {
            return Ok(self.0.as_mut().unwrap());
        }

        let dolr_treasury_limit = storage
            .get("dolr-treasury-limit")
            .await?
            .unwrap_or_default();
        self.0 = Some(dolr_treasury_limit);
        Ok(self.0.as_mut().unwrap())
    }

    async fn treasury(&mut self, storage: &mut SafeStorage) -> Result<&mut DolrTreasuryInner> {
        let treasury = self.get_or_init(storage).await?;
        if Date::now().as_millis() - (24 * 3600 * 1000) >= treasury.last_reset_epoch {
            *treasury = DolrTreasuryInner::default();
            storage.put("dolr-treasury-limit", treasury).await?;
        };

        Ok(treasury)
    }

    pub async fn try_consume(&mut self, storage: &mut SafeStorage, amount: Nat) -> Result<()> {
        let treasury = self.treasury(storage).await?;
        if treasury.amount.clone() < amount.0 {
            return Err(worker::Error::RustError("daily limit reached".into()));
        }
        treasury.amount -= amount.0;
        storage.put("dolr-treasury-limit", treasury).await?;

        Ok(())
    }

    pub async fn rollback(&mut self, storage: &mut SafeStorage, amount: Nat) -> Result<()> {
        let treasury = self.treasury(storage).await?;
        treasury.amount =
            (treasury.amount.clone() + amount.0).min(MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER.into());
        storage.put("dolr-treasury-limit", treasury).await?;

        Ok(())
    }

    pub async fn amount(&mut self, storage: &mut SafeStorage) -> Result<Nat> {
        self.treasury(storage)
            .await
            .map(|v| v.amount.clone().into())
    }
}
