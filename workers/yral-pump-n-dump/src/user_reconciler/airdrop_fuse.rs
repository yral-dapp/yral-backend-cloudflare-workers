use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::js_sys::Date as JsDate;

/// A fuse which is reset at 12 AM UTC
#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct AirdropFuse {
    next_claim_epoch_ms: f64,
}

impl AirdropFuse {
    pub fn can_claim(&mut self) -> bool {
        let now = JsDate::now();
        if now < self.next_claim_epoch_ms {
            return false;
        }

        let next_claim = JsDate::new_0();
        next_claim.set_utc_date(next_claim.get_utc_date() + 1);
        next_claim.set_utc_hours(0);
        next_claim.set_utc_minutes(0);
        next_claim.set_utc_seconds(0);
        next_claim.set_utc_milliseconds(0);

        self.next_claim_epoch_ms = next_claim.get_time();
        true
    }
}
