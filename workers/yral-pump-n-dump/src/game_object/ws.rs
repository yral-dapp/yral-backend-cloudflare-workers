use candid::Principal;
use pump_n_dump_common::ws::{GameResult, WsMessage, WsRequest, WsResp, WsResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use worker::{Result, WebSocket, WebSocketIncomingMessage};

use crate::game_object::GameObjReq;

use super::GameState;

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
        let event = WsResp::GameResultEvent(game_result);
        self.broadcast_event(event)
    }

    pub fn broadcast_pool_update(&self, new_pool: u64) -> Result<()> {
        let event = WsResp::WinningPoolEvent(new_pool);
        self.broadcast_event(event)
    }

    fn broadcast_event(&self, resp: WsResp) -> Result<()> {
        let resp = WsResponse {
            request_id: Uuid::max(),
            response: resp,
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
