use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repository {
  pub id: i64,
  pub owner: String,
  pub repo: String,
}

impl<'de> Deserialize<'de> for Repository {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    struct Owner {
      login: String,
    }
    #[derive(Deserialize)]
    struct ReceivedRepository {
      id: i64,
      owner: Owner,
      name: String,
    }
    let ReceivedRepository { id, owner, name } = ReceivedRepository::deserialize(deserializer)?;

    Ok(Repository {
      id,
      owner: owner.login,
      repo: name,
    })
  }
}

impl fmt::Display for Repository {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}/{}", self.owner, self.repo)
  }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum PrState {
  Open,
  Closed,
}

#[derive(Debug)]
pub struct PullRequest {
  pub state: PrState,
  pub merged: bool,
  pub draft: bool,
  pub commit_hash: String,
}

impl<'de> Deserialize<'de> for PullRequest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    struct Head {
      sha: String,
    }
    #[derive(Deserialize)]
    struct RPullRequest {
      state: PrState,
      merged: bool,
      draft: bool,
      head: Head,
    }
    let RPullRequest {
      state,
      merged,
      draft,
      head,
    } = RPullRequest::deserialize(deserializer)?;

    Ok(PullRequest {
      state,
      merged,
      draft,
      commit_hash: head.sha,
    })
  }
}
