use crate::control::command::{Command, Context};
use crate::github::client::{Client, Credentials, TokenCache};
use crate::github::types::Repository;
use crate::github::CommandContext;

use std::sync::Arc;

use actix_web::client::Client as AwcClient;
use log::{error, info};
use serde::Deserialize;
use tokio::sync::Mutex;

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
  pub number: i64,
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
  pub repository: Repository,
}

pub(super) async fn handle(data: T, credentials: Credentials, token_cache: Arc<Mutex<TokenCache>>) {
  match data.action {
    Action::Created => {}
    _ => {
      return;
    }
  }
  let mut context = CommandContext {
    client: Client::new(credentials, token_cache, AwcClient::new()),
    repository: data.repository,
    issue_number: data.issue.number,
  };
  let commands = match Command::parse_comment(&data.comment.body[..]) {
    Ok(commands) => commands,
    Err(e) => {
      let error_message = format!("Error: {}", e);
      match context.reply(error_message).await {
        Ok(()) => {}
        Err(e) => {
          error!("sending error message: {}", e);
        }
      }
      return;
    }
  };
  if commands.is_empty() {
    return;
  }
  info!("received commands: {:?}", commands);
  for command in commands {
    match command.run(&mut context).await {
      Ok(_) => {}
      Err(e) => {
        let error_message = format!("Error running command: {}: {}", command, e,);
        match context.reply(error_message).await {
          Ok(()) => {}
          Err(e) => {
            error!("sending error message: {}", e);
          }
        }
        return;
      }
    }
  }
}
