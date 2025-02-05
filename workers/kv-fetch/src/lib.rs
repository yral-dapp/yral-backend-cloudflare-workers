use serde_json::json;
use worker::*;

mod utils;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router
        .post_async("/:key", |mut req, ctx| async move {
            let req_auth_key = match req.headers().get("Authorization") {
                Err(err) => return Response::error(format!("invalid header: {}", err), 401),
                Ok(None) => return Response::error("unauthorized", 400),
                Ok(Some(key)) => key,
            };

            let auth_key = match ctx.secret("SET_VALUE_API_KEY") {
                Err(err) => {
                    return Response::error(format!("error fetching auth key: {}", err), 500)
                }
                Ok(auth_key) => auth_key.to_string(),
            };

            if req_auth_key != auth_key {
                return Response::error("unauthorized", 400);
            }

            let Some(key) = ctx.param("key") else {
                return Response::error("expected key and value path params", 400);
            };

            let value = match req.text().await {
                Ok(value) => value,
                Err(err) => return Response::error(format!("unable to parse value: {}", err), 400),
            };

            if value.is_empty() {
                return Response::error("value cannot be empty", 400);
            }

            match ctx.kv("kvfetch")?.put(key, &value)?.execute().await {
                Ok(_) => Response::ok(
                    json! ({
                        "key": key,
                        "value": value
                    })
                    .to_string(),
                ),
                Err(err) => Response::error(format!("unable to set value: {}", err), 500),
            }
        })
        .get_async("/:key", |_req, ctx| async move {
            let Some(key) = ctx.param("key") else {
                return Response::error("expected key path param", 400);
            };

            match ctx.kv("kvfetch")?.get(key).text().await? {
                Some(value) => Response::ok(value),
                None => Response::error("key not found", 404),
            }
        })
        .run(req, env)
        .await
}
