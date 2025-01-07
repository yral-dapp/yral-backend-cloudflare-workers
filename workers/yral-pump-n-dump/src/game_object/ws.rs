use candid::{Nat, Principal};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use worker::{Result, WebSocket, WebSocketIncomingMessage};

use crate::{game_object::GameObjReq, utils::GameDirection};

use super::GameState;

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize)]
pub enum WsMessage {
    Bet(GameDirection),
}

#[derive(Serialize, Deserialize)]
pub struct WsRequest {
    pub request_id: Uuid,
    pub msg: WsMessage,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GameResult {
    pub direction: GameDirection,
    pub reward_pool: Nat,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum WsResp {
    Ok,
    Error(String),
    GameResult(GameResult),
}

#[derive(Serialize, Deserialize)]
pub struct WsResponse {
    pub request_id: Uuid,
    pub response: WsResp,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct WsState {
    game_canister: Principal,
    token_root: Principal,
    user_canister: Principal,
}

impl GameState {
    pub fn handle_ws(
        &self,
        ws: WebSocket,
        game_canister: Principal,
        token_root: Principal,
        user_canister: Principal,
    ) -> Result<()> {
        self.state.accept_web_socket(&ws);
        ws.serialize_attachment(WsState {
            game_canister,
            token_root,
            user_canister,
        })?;

        Ok(())
    }

    pub fn broadcast_game_result(&self, game_result: GameResult) -> Result<()> {
        let event = WsResp::GameResult(game_result);
        let resp = WsResponse {
            request_id: Uuid::max(),
            response: event,
        };
        for ws in self.state.get_websockets() {
            ws.send(&resp)?;
        }

        Ok(())
    }

    pub async fn handle_ws_message(
        &mut self,
        ws: &WebSocket,
        msg: WebSocketIncomingMessage,
    ) -> Result<WsResponse> {
        let WebSocketIncomingMessage::String(raw_msg) = msg else {
            return Ok(WsResponse {
                request_id: Uuid::nil(),
                response: WsResp::Error("unknown request".into()),
            });
        };
        let Ok(ws_req) = serde_json::from_str::<WsRequest>(&raw_msg) else {
            return Ok(WsResponse {
                request_id: Uuid::nil(),
                response: WsResp::Error("unknown request".into()),
            });
        };
        let state: WsState = ws.deserialize_attachment()?.unwrap();
        let WsMessage::Bet(direction) = ws_req.msg;

        let res = self
            .game_request(GameObjReq {
                sender: state.user_canister,
                direction,
                creator: state.game_canister,
                token_root: state.token_root,
            })
            .await;

        if let Err(e) = res {
            return Ok(WsResponse {
                request_id: ws_req.request_id,
                response: WsResp::Error(e.to_string()),
            });
        }

        Ok(WsResponse {
            request_id: ws_req.request_id,
            response: WsResp::Ok,
        })
    }
}
