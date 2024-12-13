pub use console_error_panic_hook::set_once as set_panic_hook;
use worker::*;

#[event(scheduled)]
pub async fn scheduled_event(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    console_log!("Hello from a scheduled event!");
    set_panic_hook();

    // Create a new request
    let req = Request::new(
        "https://icp-off-chain-agent.fly.dev/update-global-ml-feed-cache",
        Method::Get,
    )
    .unwrap();

    // Make the HTTP request using Fetch API
    match Fetch::Request(req).send().await {
        Ok(resp) => {
            // Log the response status
            console_log!("Request completed with status: {}", resp.status_code());
        }
        Err(e) => {
            console_error!("Request failed: {}", e);
        }
    }
}
