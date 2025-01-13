use serde::Serialize;
use wasm_bindgen_futures::wasm_bindgen;
use worker::{Headers, Method, RequestInit, Result};

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

pub const fn is_testing() -> bool {
    let Some(test_v) = option_env!("TEST") else {
        return false;
    };

    matches!(test_v.as_bytes(), b"1")
}
