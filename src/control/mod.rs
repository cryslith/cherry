use crate::github::client::Client;
use crate::github::client::ClientError;
use crate::github::types::{PrState as GHPrState, Repository};

use std::fmt;
use std::str::FromStr;

use chrono::Utc;
use futures::future::LocalBoxFuture;
use quaint::ast::{Comparable, Conjuctive, Delete, Insert, Select, Update};
use quaint::connector::{Queryable, TransactionCapable};
use thiserror::Error;

pub mod command;

enum PrState {
  Requested,
  Queued,
  Merging,
  Split,
}

impl fmt::Display for PrState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Requested => write!(f, "requested"),
      Self::Queued => write!(f, "queued"),
      Self::Merging => write!(f, "merging"),
      Self::Split => write!(f, "split"),
    }
  }
}

impl FromStr for PrState {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "requested" => Ok(Self::Requested),
      "queued" => Ok(Self::Queued),
      "merging" => Ok(Self::Merging),
      "split" => Ok(Self::Split),
      _ => Err(s.to_string()),
    }
  }
}

#[derive(Debug, Error)]
pub enum ControllerError {
  #[error(transparent)]
  Client(#[from] ClientError),
  #[error(transparent)]
  DB(#[from] quaint::error::Error),
}

pub struct Controller<Q>
where
  Q: Queryable + TransactionCapable + 'static,
{
  client: Client,
  db: Q,
}

impl<Q> Controller<Q>
where
  Q: Queryable + TransactionCapable + 'static,
{
  pub async fn request(&self, repo: &Repository, pr: i64) -> Result<(), ControllerError> {
    let pr_info = self.client.pr_info(repo, pr).await?;

    match pr_info.state {
      GHPrState::Open => (),
      GHPrState::Closed => {
        self
          .client
          .comment_on_pr(repo, pr, "Error: Refusing to merge PR in closed state.")
          .await?;
        return Ok(());
      }
    }

    // TODO readiness check
    let ready = !pr_info.draft;

    let state = if ready {
      PrState::Queued
    } else {
      PrState::Requested
    };
    match self
      .db
      .insert(
        Insert::single_into("pull_request")
          .value("owner", repo.owner.as_str())
          .value("repo", repo.repo.as_str())
          .value("number", pr)
          .value("commit_hash", pr_info.commit_hash)
          .value("state", state.to_string())
          .value("timestamp", Utc::now().timestamp())
          .build(),
      )
      .await
    {
      Ok(_) => (),
      Err(quaint::error::Error::UniqueConstraintViolation { .. }) => {
        self
          .client
          .comment_on_pr(repo, pr, "This PR is already being merged.")
          .await?;
        return Ok(());
      }
      Err(e) => {
        return Err(e.into());
      }
    }
    if ready {
      self.construct().await;
    } else {
      // TODO list applicable conditions
      self
        .client
        .comment_on_pr(repo, pr, "This PR cannot be merged yet.  It will be merged automatically once the following conditions are resolved:\n- PR not marked as draft")
        .await?;
    }
    Ok(())
  }

  pub async fn initiate(&self, repo: &Repository, pr: i64) -> Result<(), ControllerError> {
    let pr_info = self.client.pr_info(repo, pr).await?;

    match pr_info.state {
      GHPrState::Open => (),
      GHPrState::Closed => {
        self
          .db
          .delete(
            Delete::from_table("pull_request").so_that(
              "owner"
                .equals(repo.owner.as_str())
                .and("repo".equals(repo.repo.as_str()))
                .and("number".equals(pr)),
            ),
          )
          .await?;
        return Ok(());
      }
    }

    // TODO readiness check
    let ready = !pr_info.draft;
    if !ready {
      return Ok(());
    }

    let tx = self.db.start_transaction().await?;
    let rows = tx
      .select(
        Select::from_table("pull_request")
          .so_that("owner".equals(repo.owner.as_str()))
          .and_where("repo".equals(repo.repo.as_str()))
          .and_where("number".equals(pr)),
      )
      .await?;
    let row = match rows.first() {
      Some(row) => row,
      None => return Ok(()),
    };

    match row["state"].as_str().unwrap().parse().unwrap() {
      PrState::Requested => (),
      _ => return Ok(()),
    }

    if row["commit_hash"].as_str().unwrap() != pr_info.commit_hash {
      tx.delete(
        Delete::from_table("pull_request").so_that(
          "owner"
            .equals(repo.owner.as_str())
            .and("repo".equals(repo.repo.as_str()))
            .and("number".equals(pr)),
        ),
      )
      .await?;
      tx.commit().await?;
      todo!("report cancelled by changing commit hash");
    }

    tx.update(
      Update::table("pull_request")
        .set("state", PrState::Queued.to_string())
        .set("timestamp", Utc::now().timestamp())
        .so_that(
          "owner"
            .equals(repo.owner.as_str())
            .and("repo".equals(repo.repo.as_str()))
            .and("number".equals(pr)),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(())
  }

  pub async fn construct(&self) {
    todo!()
  }

  pub async fn test(&self) {
    todo!()
  }

  pub fn complete(&self) -> LocalBoxFuture<'_, ()> {
    todo!()
  }

  pub async fn cancel(&self) {
    todo!()
  }

  pub async fn poll(&self) {
    todo!()
  }
}
