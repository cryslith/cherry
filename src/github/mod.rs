use crate::control::command::Context;
use client::{Client, ClientError};
use types::Repository;

use async_trait::async_trait;
use thiserror::Error;

pub mod client;
pub mod types;
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
    self.client.comment_on_pr(&self.repository, self.issue_number, message.as_str()).await
      .map_err(Into::into)
  }
}
