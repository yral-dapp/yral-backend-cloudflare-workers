#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunEnv {
    Mock,
    Local,
    Remote,
}

pub const fn env_kind() -> RunEnv {
    let Some(test_v) = option_env!("ENV") else {
        return RunEnv::Remote;
    };

    match test_v.as_bytes() {
        b"mock" | b"MOCK" => RunEnv::Mock,
        b"local" | b"LOCAL" => RunEnv::Local,
        _ => RunEnv::Remote,
    }
}
