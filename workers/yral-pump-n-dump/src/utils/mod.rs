use std::fmt::Debug;

use candid::Principal;
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen_futures::wasm_bindgen;
use worker::{Headers, Method, RequestInit, Result, RouteContext, Storage, Stub};

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

pub struct StorageCell<T: Serialize + DeserializeOwned + Clone + Debug> {
    key: String,
    hot_cache: Option<T>,
    initial_value: Option<Box<dyn FnOnce() -> T>>,
}

impl<T: Serialize + DeserializeOwned + Clone + Debug> StorageCell<T> {
    pub fn new(key: impl AsRef<str>, initial_value: impl FnOnce() -> T + 'static) -> Self {
        Self {
            key: key.as_ref().to_string(),
            hot_cache: None,
            initial_value: Some(Box::new(initial_value)),
        }
    }

    pub async fn set(&mut self, storage: &mut Storage, v: T) -> worker::Result<()> {
        worker::console_log!("new value for obj {:?}", v);
        self.hot_cache = Some(v.clone());
        storage.put(&self.key, v).await
    }

    pub async fn update(
        &mut self,
        storage: &mut Storage,
        updater: impl FnOnce(&mut T),
    ) -> worker::Result<()> {
        let mutated_val = if let Some(v) = self.hot_cache.as_mut() {
            v
        } else {
            let stored_val = storage.get(&self.key).await.unwrap_or_else(|e| {
                worker::console_log!("failed to get obj: {e}");
                (self
                    .initial_value
                    .take()
                    .expect("initial value borrow error"))()
            });
            self.hot_cache = Some(stored_val);
            self.hot_cache.as_mut().unwrap()
        };
        updater(mutated_val);
        worker::console_log!("new value for obj {:?}", mutated_val);

        storage.put(&self.key, mutated_val.clone()).await?;

        Ok(())
    }

    pub async fn read(&mut self, storage: &Storage) -> &T {
        if self.hot_cache.is_some() {
            return self.hot_cache.as_ref().unwrap();
        }

        let stored_val = storage.get(&self.key).await.unwrap_or_else(|e| {
            worker::console_log!("failed to get obj: {e}");
            (self
                .initial_value
                .take()
                .expect("initial value borrow error"))()
        });
        self.hot_cache = Some(stored_val);

        self.hot_cache.as_ref().unwrap()
    }
}
