pub use console_error_panic_hook::set_once as set_panic_hook;
use worker::*;

#[event(scheduled)]
pub async fn scheduled_event(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    console_log!("Hello from a scheduled event!");
    set_panic_hook();

    // Create a new request
    let req = Request::new(
        "https://yral-ml-feed-server.fly.dev/api/v1/feed/global-cache/clean",
        Method::Post,
    )
    .unwrap();

    // Make the HTTP request using Fetch API
    match Fetch::Request(req).send().await {
        Ok(resp) => {
            // Log the response status
            console_log!(
                "Clean Request completed with status: {}",
                resp.status_code()
            );
        }
        Err(e) => {
            console_error!("Clean Request failed: {}", e);
        }
    }

    // Create a new request
    let req = Request::new(
        "https://yral-ml-feed-server.fly.dev/api/v1/feed/global-cache/mixed",
        Method::Post,
    )
    .unwrap();

    // Make the HTTP request using Fetch API
    match Fetch::Request(req).send().await {
        Ok(resp) => {
            // Log the response status
            console_log!(
                "Mixed Request completed with status: {}",
                resp.status_code()
            );
        }
        Err(e) => {
            console_error!("Mixed Request failed: {}", e);
        }
    }

    // Create a new request
    let req = Request::new(
        "https://yral-ml-feed-server.fly.dev/api/v1/feed/global-cache/nsfw",
        Method::Post,
    )
    .unwrap();

    // Make the HTTP request using Fetch API
    match Fetch::Request(req).send().await {
        Ok(resp) => {
            // Log the response status
            console_log!("NSFW Request completed with status: {}", resp.status_code());
        }
        Err(e) => {
            console_error!("NSFW Request failed: {}", e);
        }
    }
}
