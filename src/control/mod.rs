use crate::github::client::Client;
use crate::github::client::ClientError;
use crate::github::types::{PrState as GHPrState, Repository};

use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::str::FromStr;

use chrono::Utc;
use futures::future::LocalBoxFuture;
use log::info;
use quaint::ast::{Comparable, Conjuctive, Delete, Insert, ParameterizedValue, Select, Update};
use quaint::connector::{Queryable, TransactionCapable};
use thiserror::Error;

pub mod command;

#[derive(Debug, Clone, Copy)]
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
  type Err = ControllerError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "requested" => Ok(Self::Requested),
      "queued" => Ok(Self::Queued),
      "merging" => Ok(Self::Merging),
      "split" => Ok(Self::Split),
      _ => Err(ControllerError::InvalidPrState(s.to_string())),
    }
  }
}

impl<'a> Into<ParameterizedValue<'a>> for PrState {
  fn into(self) -> ParameterizedValue<'a> {
    self.to_string().into()
  }
}

impl<'a> TryFrom<&ParameterizedValue<'a>> for PrState {
  type Error = ControllerError;

  fn try_from(v: &ParameterizedValue<'a>) -> Result<Self, Self::Error> {
    v.as_str()
      .ok_or(ControllerError::InvalidPrState("not a string".to_string()))?
      .parse()
  }
}

#[derive(Debug, Clone, Copy)]
enum MergeState {
  Constructing,
  Testing,
  Success,
  Split,
}

impl fmt::Display for MergeState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Constructing => write!(f, "constructing"),
      Self::Testing => write!(f, "testing"),
      Self::Success => write!(f, "success"),
      Self::Split => write!(f, "split"),
    }
  }
}

impl FromStr for MergeState {
  type Err = ControllerError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "constructing" => Ok(Self::Constructing),
      "testing" => Ok(Self::Testing),
      "success" => Ok(Self::Success),
      "split" => Ok(Self::Split),
      _ => Err(ControllerError::InvalidMergeState(s.to_string())),
    }
  }
}

impl<'a> Into<ParameterizedValue<'a>> for MergeState {
  fn into(self) -> ParameterizedValue<'a> {
    self.to_string().into()
  }
}

impl<'a> TryFrom<&ParameterizedValue<'a>> for MergeState {
  type Error = ControllerError;

  fn try_from(v: &ParameterizedValue<'a>) -> Result<Self, Self::Error> {
    v.as_str()
      .ok_or(ControllerError::InvalidMergeState(
        "not a string".to_string(),
      ))?
      .parse()
  }
}

#[derive(Debug, Error)]
pub enum ControllerError {
  #[error(transparent)]
  Client(#[from] ClientError),
  #[error(transparent)]
  DB(#[from] quaint::error::Error),
  #[error("invalid PR state: {0}")]
  InvalidPrState(String),
  #[error("invalid merge state: {0}")]
  InvalidMergeState(String),
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
    info!("request: {} #{}", repo, pr);
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
          .value("state", state)
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
    info!("added {} #{} in {} state", repo, pr, state);
    if ready {
      self.construct(repo).await
    } else {
      // TODO list applicable conditions
      self
        .client
        .comment_on_pr(repo, pr, "This PR cannot be merged yet.  It will be merged automatically once the following conditions are resolved:\n- PR not marked as draft")
        .await?;
      Ok(())
    }
  }

  pub async fn initiate(&self, repo: &Repository, pr: i64) -> Result<(), ControllerError> {
    info!("initiate: {} #{}", repo, pr);
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

    match (&row["state"]).try_into()? {
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
      self
        .client
        .comment_on_pr(
          repo,
          pr,
          "Merge cancelled: a new commit was pushed to the PR.",
        )
        .await?;
    }

    tx.update(
      Update::table("pull_request")
        .set("state", PrState::Queued)
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
    info!("queued {} #{}", repo, pr);
    Ok(())
  }

  pub async fn construct(&self, repo: &Repository) -> Result<(), ControllerError> {
    let tx = self.db.start_transaction().await?;
    if !tx
      .select(
        Select::from_table("merge_attempt")
          .so_that("owner".equals(repo.owner.as_str()))
          .and_where("repo".equals(repo.repo.as_str()))
          .and_where("state".not_equals(MergeState::Split)),
      )
      .await?
      .is_empty()
    {
      info!("not constructing merge attempt because merge attempt is already in progress");
      return Ok(());
    }

    let split_rows = tx
      .select(
        Select::from_table("merge_attempt")
          .so_that("owner".equals(repo.owner.as_str()))
          .and_where("repo".equals(repo.repo.as_str()))
          .and_where("state".equals(MergeState::Split)),
      )
      .await?;

    let id = if let Some(split_row) = split_rows.first() {
      let id = split_row["id"].as_str().unwrap();
      tx.update(
        Update::table("merge_attempt")
          .set("state", MergeState::Constructing)
          .set("timestamp", Utc::now().timestamp())
          .so_that("id".equals(id)),
      )
      .await?;
      todo!("need to record the branch name??");
      id
    } else {
      let id = uuid::Uuid::new_v4().to_string();
      self
        .db
        .insert(
          Insert::single_into("merge_attempt")
            .value("id", id)
            .value("owner", repo.owner.as_str())
            .value("repo", repo.repo.as_str())
            .value("state", MergeState::Constructing)
            .value("timestamp", Utc::now().timestamp())
            .build(),
        )
        .await?;
      todo!("need to record the branch name??");
      id.as_str()
    };

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
