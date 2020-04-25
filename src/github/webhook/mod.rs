use actix_web::{error, http::StatusCode, web, HttpRequest, HttpResponse};
use log::trace;
use serde_json::from_slice;
use thiserror::Error;

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

  async fn handle(self) -> Result<HttpResponse, WebhookError> {
    match self {
      Self::IssueComment(d) => issue_comment::handle(d).await,
      Self::Unknown => Ok(HttpResponse::Accepted().finish()),
    }
  }
}

pub async fn webhook(request: HttpRequest, body: web::Bytes) -> Result<HttpResponse, WebhookError> {
  let headers = request.headers();
  let event_type = headers
    .get("X-GitHub-Event")
    .ok_or(WebhookError::MissingEventType)?
    .to_str()
    .map_err(|_| WebhookError::InvalidEventType)?;

  trace!("received webhook: {:?}", event_type);
  WebhookRequest::parse(event_type, &body)?.handle().await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_webhook_parse() {
    use WebhookRequest::*;
    {
      use issue_comment::*;
      assert_eq!(
        IssueComment(T {
          action: Action::Created,
          issue: Issue {
            state: State::Open,
            pull_request: None,
          },
          comment: Comment {
            user: User {
              login: "Codertocat".to_string(),
            },
            body: "You are totally right! I'll get this fixed right away.".to_string(),
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

mod issue_comment {
  use super::WebhookError;
  use crate::control::command::Command;

  use actix_web::HttpResponse;
  use log::{error, info};
  use serde::Deserialize;

  #[derive(Debug, Deserialize, PartialEq)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum Action {
    Created,
    Edited,
    Deleted,
  }

  #[derive(Debug, Deserialize, PartialEq)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum State {
    Open,
    Closed,
  }

  #[derive(Debug, Deserialize, PartialEq)]
  pub(super) struct PullRequest;

  #[derive(Debug, Deserialize, PartialEq)]
  pub(super) struct Issue {
    pub state: State,
    pub pull_request: Option<PullRequest>,
  }

  #[derive(Debug, Deserialize, PartialEq)]
  pub(super) struct User {
    pub login: String,
  }

  #[derive(Debug, Deserialize, PartialEq)]
  pub(super) struct Comment {
    pub user: User,
    pub body: String,
  }

  #[derive(Debug, Deserialize, PartialEq)]
  pub(super) struct T {
    pub action: Action,
    pub issue: Issue,
    pub comment: Comment,
  }

  pub(super) async fn handle(data: T) -> Result<HttpResponse, WebhookError> {
    match data.action {
      Action::Created => {}
      _ => {
        return Ok(HttpResponse::Accepted().finish());
      }
    }
    let commands = match Command::parse_comment(&data.comment.body[..]) {
      Ok(commands) => commands,
      Err(e) => {
        error!("failed to parse comment: {:?}", e);
        // TODO send error message to user
        return Ok(HttpResponse::Ok().finish());
      }
    };
    if commands.is_empty() {
      return Ok(HttpResponse::Accepted().finish());
    }
    info!("received commands: {:?}", commands);
    return Ok(HttpResponse::Ok().finish());
  }
}
