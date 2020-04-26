use crate::control::command::Context;
use client::{Client, Repository};

use actix_web::http::Method;
use async_trait::async_trait;
use serde_json::json;

pub mod client;
pub mod webhook;

pub struct CommandContext {
  client: Client,
  repository: Repository,
  issue_number: i64,
}

#[async_trait(?Send)]
impl Context for CommandContext {
  async fn reply(&mut self, message: String) {
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
      .unwrap(); // FIXME
    let mut response = self
      .client
      .app_request(Method::POST, uri)
      .unwrap() // FIXME
      .send_json(&json!({
        "body": message,
      }))
      .await
      .unwrap(); // FIXME
    Client::response_ok(&mut response).await.unwrap(); // FIXME
  }
}
