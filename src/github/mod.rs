use crate::control::command::Context;
use client::{Client, ClientError, Repository};

use actix_web::http::Method;
use async_trait::async_trait;
use serde_json::json;
use thiserror::Error;

pub mod client;
pub mod webhook;

#[derive(Debug, Error)]
pub enum CommandError {
  #[error("client operation")]
  Client(#[from] ClientError),
}

pub struct CommandContext {
  client: Client,
  repository: Repository,
  issue_number: i64,
}

#[async_trait(?Send)]
impl Context for CommandContext {
  type Error = CommandError;

  async fn reply(&mut self, message: String) -> Result<(), Self::Error> {
    let uri = self
      .client
      .api()
      .path_and_query(
        format!(
          "/repos/{}/issues/{}/comments",
          self.repository, self.issue_number
        )
        .as_str(),
      )
      .build()
      .map_err::<ClientError, _>(Into::into)?;
    let mut response = self
      .client
      .repo_request(self.repository.clone(), Method::POST, uri)
      .await?
      .send_json(&json!({
        "body": message,
      }))
      .await
      .map_err::<ClientError, _>(Into::into)?;
    Client::response_ok(&mut response).await?;
    Ok(())
  }
}
