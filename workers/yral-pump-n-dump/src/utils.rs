use serde::{Deserialize, Serialize};

pub const fn is_testing() -> bool {
    let Some(test_v) = option_env!("TEST") else {
        return false;
    };

    matches!(test_v.as_bytes(), b"1")
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum GameDirection {
    Pump,
    Dump,
}
