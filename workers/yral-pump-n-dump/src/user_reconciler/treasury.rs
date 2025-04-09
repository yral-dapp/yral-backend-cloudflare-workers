use candid::Nat;
use serde::{Deserialize, Serialize};
use worker::{Date, Result};

use crate::{consts::MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER, utils::storage::SafeStorage};

#[derive(Serialize, Deserialize, Clone)]
struct DolrTreasuryInner {
    amount: Nat,
    last_reset_epoch: u64,
}

impl Default for DolrTreasuryInner {
    fn default() -> Self {
        Self {
            amount: Nat::from(MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER),
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
        // if last set epoch is greater than 24 hours from now
        if treasury.last_reset_epoch >= Date::now().as_millis() + (24 * 3600 * 1000) {
            *treasury = DolrTreasuryInner::default();
            storage.put("dolr-treasury-limit", treasury).await?;
        };

        Ok(treasury)
    }

    pub async fn try_consume(&mut self, storage: &mut SafeStorage, amount: Nat) -> Result<()> {
        let treasury = self.treasury(storage).await?;
        if treasury.amount.clone() < amount {
            return Err(worker::Error::RustError("daily limit reached".into()));
        }
        treasury.amount -= amount;
        storage.put("dolr-treasury-limit", treasury).await?;

        Ok(())
    }

    pub async fn rollback(&mut self, storage: &mut SafeStorage, amount: Nat) -> Result<()> {
        let treasury = self.treasury(storage).await?;
        treasury.amount =
            (treasury.amount.clone() + amount).min(MAXIMUM_DOLR_TREASURY_PER_DAY_PER_USER.into());
        storage.put("dolr-treasury-limit", treasury).await?;

        Ok(())
    }

    pub async fn amount(&mut self, storage: &mut SafeStorage) -> Result<Nat> {
        self.treasury(storage).await.map(|v| v.amount.clone())
    }
}
