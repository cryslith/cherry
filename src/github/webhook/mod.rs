use crate::github::client::{Credentials, TokenCache};

use std::sync::Arc;

use actix_rt::spawn;
use actix_web::{error, http::StatusCode, web, HttpRequest, HttpResponse};
use log::trace;
use serde_json::from_slice;
use thiserror::Error;
use tokio::sync::Mutex;

mod issue_comment;

#[derive(Debug, Error)]
pub enum WebhookError {
  #[error("missing event type header")]
  MissingEventType,
  #[error("invalid event type header")]
  InvalidEventType,
  #[error("failed to deserialize webhook payload")]
  PayloadDeserialization(#[from] serde_json::Error),
}

impl error::ResponseError for WebhookError {
  fn status_code(&self) -> StatusCode {
    match self {
      Self::MissingEventType | Self::InvalidEventType | Self::PayloadDeserialization(_) => {
        StatusCode::BAD_REQUEST
      }
    }
  }
}

#[derive(Debug, PartialEq)]
enum WebhookRequest {
  IssueComment(issue_comment::T),
  Unknown,
}

impl WebhookRequest {
  fn parse(event_type: &str, body: &[u8]) -> Result<Self, WebhookError> {
    match event_type {
      "issue_comment" => Ok(Self::IssueComment(from_slice(&body)?)),
      _ => Ok(Self::Unknown),
    }
  }

  async fn handle(self, credentials: Credentials, token_cache: Arc<Mutex<TokenCache>>) {
    match self {
      Self::IssueComment(d) => issue_comment::handle(d, credentials, token_cache).await,
      Self::Unknown => {}
    }
  }
}

pub async fn webhook(
  request: HttpRequest,
  body: web::Bytes,
  credentials: web::Data<Credentials>,
  token_cache: web::Data<Arc<Mutex<TokenCache>>>,
) -> Result<HttpResponse, WebhookError> {
  let headers = request.headers();
  let event_type = headers
    .get("X-GitHub-Event")
    .ok_or(WebhookError::MissingEventType)?
    .to_str()
    .map_err(|_| WebhookError::InvalidEventType)?;

  trace!("received webhook: {:?}", event_type);
  let request = WebhookRequest::parse(event_type, &body)?;
  spawn(request.handle(credentials.as_ref().clone(), token_cache.as_ref().clone()));
  Ok(HttpResponse::Accepted().finish())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_webhook_parse() {
    use WebhookRequest::*;
    {
      use crate::github::client::Repository;
      use issue_comment::*;
      assert_eq!(
        IssueComment(T {
          action: Action::Created,
          issue: Issue {
            number: 1,
            state: State::Open,
            pull_request: None,
          },
          comment: Comment {
            user: User {
              login: "Codertocat".to_string(),
            },
            body: "You are totally right! I'll get this fixed right away.".to_string(),
          },
          repository: Repository {
            id: 186853002,
            owner: "Codertocat".to_string(),
            repo: "Hello-World".to_string(),
          }
        }),
        WebhookRequest::parse(
          "issue_comment",
          include_bytes!("test_data/parse/00_issue_comment.json")
        )
        .unwrap(),
      );
    }
    assert_eq!(Unknown, WebhookRequest::parse("nyanyan", b"").unwrap(),);
  }
}
