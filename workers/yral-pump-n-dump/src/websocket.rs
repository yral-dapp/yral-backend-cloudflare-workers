use candid::{CandidType, Principal};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::result::Result as StdResult;
use uuid::Uuid;
use worker::{Env, Method, Request, RequestInit, Result, Stub, WebSocket, WebSocketPair};
use yral_identity::{msg_builder, Signature};

use crate::{
    backend_impl::{WsBackend, WsBackendImpl},
    game_object::{GameObjReq, GameResult},
};

#[derive(Serialize, Deserialize, CandidType, Clone, Copy)]
pub enum GameDirection {
    Pump,
    Dump,
}

#[derive(Serialize, Deserialize)]
pub struct IdentifyReq {
    // principal of the user playing the game
    pub sender: Principal,
    // root of the token
    pub token_root: Principal,
    // principal of the token creator's canister
    pub game_canister: Principal,
    // signature verifying the user's consent
    pub signature: Signature,
}

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize)]
pub enum WsMessage {
    Identify(IdentifyReq),
    Bet(GameDirection),
}

#[derive(Serialize, Deserialize)]
pub struct WsRequest {
    pub request_id: Uuid,
    pub msg: WsMessage,
}

#[derive(Serialize, Deserialize)]
pub enum WsResp {
    Identified,
    AuthenticationRequired,
    GameRes(GameResult),
    Error(String),
}

#[derive(Serialize, Deserialize)]
pub struct WsResponse {
    pub request_id: Uuid,
    pub response: WsResp,
}

fn verify_identify_req(req: &IdentifyReq) -> StdResult<(), String> {
    let msg = msg_builder::Message::default()
        .method_name("pump_or_dump_worker".into())
        .args((req.game_canister, req.token_root))
        .expect("Game request should serialize");

    let verify_res = req.signature.clone().verify_identity(req.sender, msg);
    if verify_res.is_err() {
        return Err("invalid signature".into());
    }

    Ok(())
}

struct WsState {
    game_stub: Stub,
    sender_canister: Principal,
    token_root: Principal,
    game_canister: Principal,
}

impl WsState {
    pub async fn new(env: &Env, req: IdentifyReq) -> Result<Self> {
        let ws_backend = WsBackend::new(env)?;
        let Some(user_canister) = ws_backend
            .user_principal_to_user_canister(req.sender)
            .await?
        else {
            return Err(worker::Error::RustError("invalid canister".into()));
        };

        let token_valid = ws_backend
            .validate_token(req.token_root, req.game_canister)
            .await?;
        if !token_valid {
            return Err(worker::Error::RustError("invalid token".into()));
        }

        let game_state = env.durable_object("GAME_STATE")?;
        let game_state_obj =
            game_state.id_from_name(&format!("{}-{}", req.game_canister, req.token_root))?;
        let game_stub = game_state_obj.get_stub()?;

        Ok(Self {
            game_stub,
            sender_canister: user_canister,
            token_root: req.token_root,
            game_canister: req.game_canister,
        })
    }

    pub async fn perform_bet(&self, direction: GameDirection) -> Result<GameResult> {
        let body = GameObjReq {
            sender: self.sender_canister,
            direction,
            creator: self.game_canister,
            token_root: self.token_root,
        };
        let mut req_init = RequestInit::new();
        let req = Request::new_with_init(
            "http://fake_url.com/bet",
            req_init
                .with_method(Method::Post)
                .with_body(Some(serde_wasm_bindgen::to_value(&body)?)),
        )?;
        let mut res = self.game_stub.fetch_with_request(req).await?;
        let result: GameResult = res.json().await?;

        Ok(result)
    }
}

async fn websocket_loop(server: WebSocket, env: Env) {
    let mut events = server.events().expect("could not open stream");

    let mut state = None::<WsState>;

    while let Some(ev) = events.next().await {
        let Ok(ev) = ev else {
            panic!("received error in ws stream!");
        };
        let msg_ev = match ev {
            worker::WebsocketEvent::Close(_) => break,
            worker::WebsocketEvent::Message(m) => m,
        };

        let Ok(req) = msg_ev.json::<WsRequest>() else {
            server
                .close(Some(400), Some("received unexpected message"))
                .expect("failed to close ws");
            return;
        };

        match req.msg {
            WsMessage::Bet(direction) => {
                let Some(state) = state.as_ref() else {
                    let resp = WsResponse {
                        request_id: req.request_id,
                        response: WsResp::AuthenticationRequired,
                    };
                    let body = serde_json::to_string(&resp).unwrap();
                    server.send_with_str(&body).expect("ws failed to send msg");
                    continue;
                };

                let res = state.perform_bet(direction).await;
                let reply = match res {
                    Ok(r) => WsResp::GameRes(r),
                    Err(e) => WsResp::Error(e.to_string()),
                };
                let resp = WsResponse {
                    request_id: req.request_id,
                    response: reply,
                };
                let body = serde_json::to_string(&resp).unwrap();
                server.send_with_str(&body).expect("ws failed to send msg");
            }
            WsMessage::Identify(id_req) => {
                if verify_identify_req(&id_req).is_err() {
                    server
                        .close(Some(401), Some("unable to identify"))
                        .expect("failed to close ws");
                    return;
                }

                let res = match WsState::new(&env, id_req).await {
                    Ok(s) => s,
                    Err(e) => {
                        server
                            .close(Some(503), Some(&format!("unable to identify {e}")))
                            .expect("failed to close ws");
                        return;
                    }
                };

                state = Some(res);

                let resp = WsResponse {
                    request_id: req.request_id,
                    response: WsResp::Identified,
                };
                let body = serde_json::to_string(&resp).unwrap();
                server.send_with_str(&body).expect("failed to send ws msg");
            }
        }
    }
}

pub fn setup_websocket(env: Env) -> Result<WebSocket> {
    let pair = WebSocketPair::new()?;
    pair.server.accept()?;
    let server = pair.server;

    wasm_bindgen_futures::spawn_local(async move {
        websocket_loop(server, env).await;
    });

    Ok(pair.client)
}
