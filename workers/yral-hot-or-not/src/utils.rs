use hon_worker_common::WorkerError;

pub fn worker_err_to_resp(status_code: u16, e: WorkerError) -> worker::Result<worker::Response> {
    Ok(worker::Response::from_json(&e)?.with_status(status_code))
}
