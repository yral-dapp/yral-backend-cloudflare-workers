use wasm_bindgen_futures::js_sys::Date as JsDate;
use worker::*;

use crate::{
    consts::MAX_AIRDROP_USERS_PER_DAY,
    utils::storage::{SafeStorage, StorageCell},
};

#[durable_object]
pub struct AirdropCounter {
    state: State,
    env: Env,
    remaining_users: StorageCell<u32>,
}

impl AirdropCounter {
    fn storage(&self) -> SafeStorage {
        self.state.storage().into()
    }

    async fn queue_reset_remaining_users_inner(&self) -> Result<()> {
        let storage = self.state.storage();

        let now = JsDate::new_0();
        let new_time = JsDate::new_0();
        new_time.set_utc_date(new_time.get_utc_date() + 1);
        new_time.set_utc_hours(0);
        new_time.set_utc_minutes(0);
        new_time.set_utc_seconds(0);
        new_time.set_utc_milliseconds(0);

        let new_offset = (new_time.get_time() - now.get_time()).ceil() as i64;
        storage.set_alarm(new_offset).await?;

        Ok(())
    }

    async fn queue_reset_remaining_users(&self) -> Result<()> {
        let storage = self.state.storage();
        if storage.get_alarm().await?.is_some() {
            return Ok(());
        }
        self.queue_reset_remaining_users_inner().await?;

        Ok(())
    }

    /// Returns true if succesful
    async fn try_decrement_user(&mut self) -> Result<bool> {
        self.queue_reset_remaining_users().await?;

        self.remaining_users
            .update(&mut self.storage(), |v| v.checked_sub(1).is_some())
            .await
    }
}

#[durable_object]
impl DurableObject for AirdropCounter {
    fn new(state: State, env: Env) -> Self {
        console_error_panic_hook::set_once();

        Self {
            state,
            env,
            remaining_users: StorageCell::new("remaining_users", || MAX_AIRDROP_USERS_PER_DAY),
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        let env = self.env.clone();
        let router = Router::with_data(self);

        router
            .get_async("/decrement", |_, ctx| async {
                let this = ctx.data;
                let decremented = this.try_decrement_user().await?;
                if !decremented {
                    return Response::error("No more airdrops left for the day", 400);
                }

                Response::ok("done")
            })
            .run(req, env)
            .await
    }

    async fn alarm(&mut self) -> Result<Response> {
        self.remaining_users
            .set(&mut self.storage(), MAX_AIRDROP_USERS_PER_DAY)
            .await?;
        self.queue_reset_remaining_users_inner().await?;

        Response::ok("done")
    }
}
