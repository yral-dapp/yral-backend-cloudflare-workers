use candid::{Int, Nat, Principal};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use worker::*;

use crate::{
    backend_impl::{BalanceBackend, BalanceBackendImpl},
    consts::GDOLLR_TO_E8S,
};

#[derive(Serialize, Deserialize, Clone)]
pub struct AddRewardReq {
    pub amount: Nat,
    pub user_canister: Principal,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DecrementReq {
    pub user_canister: Principal,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClaimGdollrReq {
    pub user_canister: Principal,
    pub amount: Nat,
}

#[durable_object]
pub struct UserDollrBalance {
    state: State,
    env: Env,
    // effective balance = on_chain_balance + off_chain_balance_delta
    off_chain_balance_delta: Option<Int>,
    user_canister: Option<Principal>,
    backend: BalanceBackend,
}

impl UserDollrBalance {
    async fn set_user_canister(&mut self, user_canister: Principal) -> Result<()> {
        if self.user_canister.is_some() {
            return Ok(());
        }

        self.user_canister = Some(user_canister);
        self.state
            .storage()
            .put("user_canister", user_canister)
            .await?;

        Ok(())
    }

    async fn off_chain_balance_delta(&mut self) -> &mut Int {
        // if let Some syntax causes lifetime issues
        if self.off_chain_balance_delta.is_some() {
            return self.off_chain_balance_delta.as_mut().unwrap();
        }

        let off_chain_balance_delta = self
            .state
            .storage()
            .get("off_chain_balance_delta")
            .await
            .unwrap_or_default();
        self.off_chain_balance_delta = Some(off_chain_balance_delta);
        self.off_chain_balance_delta.as_mut().unwrap()
    }

    async fn effective_balance_inner(&mut self, on_chain_balance: Nat) -> Nat {
        let mut effective_balance = on_chain_balance;
        let off_chain_delta = self.off_chain_balance_delta().await.clone();
        if off_chain_delta < 0 {
            effective_balance.0 -= (-off_chain_delta.0.clone())
                .to_biguint()
                .unwrap();
        } else {
            effective_balance.0 += off_chain_delta.0.to_biguint().unwrap();
        };

        effective_balance
    }

    async fn effective_balance(&mut self, user_canister: Principal) -> Result<Nat> {
        let on_chain_balance = self.backend.gdollr_balance(user_canister).await?;

        Ok(self.effective_balance_inner(on_chain_balance).await)
    }

    async fn decrement(&mut self) -> Result<()> {
        *self.off_chain_balance_delta().await -= GDOLLR_TO_E8S;
        self.state
            .storage()
            .put(
                "off_chain_balance_delta",
                self.off_chain_balance_delta.clone(),
            )
            .await?;

        Ok(())
    }

    async fn add_reward(&mut self, amount: Nat) -> Result<()> {
        self.off_chain_balance_delta().await.0 += BigInt::from(amount.0);
        self.state
            .storage()
            .put(
                "off_chain_balance_delta",
                self.off_chain_balance_delta.clone(),
            )
            .await?;

        Ok(())
    }

    async fn settle_balance(&mut self, user_canister: Principal) -> Result<()> {
        let to_settle = self.off_chain_balance_delta().await.clone();
        self.off_chain_balance_delta = Some(0.into());
        self.state
            .storage()
            .put("off_chain_balance_delta", Nat::from(0u32))
            .await?;

        let res = self
            .backend
            .settle_gdollr_balance(user_canister, to_settle.clone())
            .await;
        if let Err(e) = res {
            self.off_chain_balance_delta = Some(to_settle.clone());
            self.state
                .storage()
                .put("off_chain_balance_delta", to_settle)
                .await?;

            return Err(e);
        }

        Ok(())
    }

    async fn claim_gdollr(&mut self, user_canister: Principal, amount: Nat) -> Result<Response> {
        let on_chain_bal = self.backend.gdollr_balance(user_canister).await?;
        if on_chain_bal >= amount {
            self.backend.redeem_gdollr(user_canister, amount).await?;
            return Response::ok("done");
        }

        let effective_bal = self.effective_balance_inner(on_chain_bal).await;
        if amount > effective_bal {
            return Response::error("not enough balance", 400);
        }

        self.settle_balance(user_canister).await?;
        self.backend.redeem_gdollr(user_canister, amount).await?;

        Response::ok("done")
    }
}

#[durable_object]
impl DurableObject for UserDollrBalance {
    fn new(state: State, env: Env) -> Self {
        let backend = BalanceBackend::new(&env).unwrap();

        // TODO: do we need balance flushing?
        Self {
            state,
            env,
            off_chain_balance_delta: None,
            user_canister: None,
            backend,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);

        router
            .get_async("/balance/:user_canister", |_req, ctx| async {
                let user_canister_raw = ctx.param("user_canister").unwrap();
                let Ok(user_canister) = Principal::from_text(user_canister_raw) else {
                    return Response::error("Invalid user_canister", 400);
                };

                let this = ctx.data;
                let bal = this.effective_balance(user_canister).await?;
                Response::ok(bal.to_string())
            })
            .post_async("/decrement", |mut req, ctx| async move {
                let this = ctx.data;
                let decr_req: DecrementReq = req.json().await?;
                this.set_user_canister(decr_req.user_canister).await?;

                let bal = this.effective_balance(decr_req.user_canister).await?;
                if bal < GDOLLR_TO_E8S {
                    return Response::error("Not enough balance", 400);
                }
                this.decrement().await?;

                Response::ok("done")
            })
            .post_async("/add_reward", |mut req, ctx| async move {
                let this = ctx.data;
                let reward_req: AddRewardReq = req.json().await?;

                this.set_user_canister(reward_req.user_canister).await?;
                this.add_reward(reward_req.amount).await?;

                Response::ok("done")
            })
            .post_async("/claim_gdollr", |mut req, ctx| async move {
                let this = ctx.data;
                let claim_req: ClaimGdollrReq = req.json().await?;

                this.set_user_canister(claim_req.user_canister).await?;

                this.claim_gdollr(claim_req.user_canister, claim_req.amount)
                    .await
            })
            .run(req, env)
            .await
    }
}
