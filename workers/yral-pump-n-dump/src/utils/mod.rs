pub mod storage;

use candid::Principal;
use serde::Serialize;
use wasm_bindgen_futures::wasm_bindgen;
use worker::{Headers, Method, RequestInit, Result, RouteContext, Stub};

#[derive(Default)]
pub struct RequestInitBuilder(RequestInit);

impl RequestInitBuilder {
    pub fn header(&mut self, k: &str, v: &str) -> Result<&mut Self> {
        self.0.headers.set(k, v)?;
        Ok(self)
    }

    pub fn replace_headers(&mut self, headers: Headers) -> &mut Self {
        self.0.headers = headers;
        self
    }

    pub fn method(&mut self, method: Method) -> &mut Self {
        self.0.method = method;
        self
    }

    // pub fn redirect(&mut self, redirect: RequestRedirect) -> &mut Self {
    //     self.0.redirect = redirect;
    //     self
    // }

    // pub fn cf_props(&mut self, props: CfProperties) -> &mut Self {
    //     self.0.cf = props;
    //     self
    // }

    pub fn json<T: Serialize>(&mut self, body: &T) -> Result<&mut Self> {
        let json = serde_json::to_string(body)?;
        self.0.body = Some(wasm_bindgen::JsValue::from_str(&json));

        self.header("Content-Type", "application/json; charset=utf-8")
    }

    pub fn build(&self) -> &RequestInit {
        &self.0
    }
}

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

macro_rules! parse_principal {
    ($ctx:ident, $param:literal) => {{
        let raw = $ctx.param($param).unwrap();
        let Ok(principal) = candid::Principal::from_text(raw) else {
            return Response::error(concat!("invalid ", $param), 400);
        };

        principal
    }};
}

pub(crate) use parse_principal;

pub fn game_state_stub<T>(
    ctx: &RouteContext<T>,
    game_canister: Principal,
    token_root: Principal,
) -> Result<Stub> {
    let game_ns = ctx.durable_object("GAME_STATE")?;
    let game_state_obj = game_ns.id_from_name(&format!("{game_canister}-{token_root}"))?;
    let game_stub = game_state_obj.get_stub()?;

    Ok(game_stub)
}

pub fn user_state_stub<T>(ctx: &RouteContext<T>, user_canister: Principal) -> Result<Stub> {
    let state_ns = ctx.durable_object("USER_EPHEMERAL_STATE")?;
    let state_obj = state_ns.id_from_name(&user_canister.to_text())?;

    state_obj.get_stub()
}
