use actix_web::{error, http::StatusCode, web, HttpRequest, HttpResponse, Responder};
use log::info;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WebhookError {
  #[error("missing event type header")]
  MissingEventType,
}

impl error::ResponseError for WebhookError {
  fn status_code(&self) -> StatusCode {
    match self {
      WebhookError::MissingEventType => StatusCode::BAD_REQUEST,
    }
  }
}

pub async fn webhook(
  request: HttpRequest,
  body: web::Bytes,
) -> Result<impl Responder, WebhookError> {
  let headers = request.headers();
  let event_type = headers
    .get("X-GitHub-Event")
    .ok_or(WebhookError::MissingEventType)?;

  info!(
    "got webhook: type = {:?}, body length = {}",
    event_type,
    body.len()
  );
  Ok(HttpResponse::Ok())
}
