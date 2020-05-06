use std::fmt;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
  #[error("unknown command: {0}")]
  UnknownCommand(String),
}

#[async_trait(?Send)]
pub trait Context {
  type Error;

  async fn reply(&mut self, message: String) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum Command {
  Ping,
  Merge,
}

impl fmt::Display for Command {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Ping => write!(f, "ping"),
      Self::Merge => write!(f, "merge"),
    }
  }
}

impl Command {
  pub fn parse_comment(s: &str) -> Result<Vec<Self>, ParseError> {
    s.lines()
      .filter_map(|l| {
        let mut words = l.split(' ');
        if words.next() != Some("cherry") {
          return None;
        }
        Some(match words.next() {
          Some("ping") => Ok(Self::Ping),
          Some("merge") | Some("r+") => Ok(Self::Merge),
          other => Err(ParseError::UnknownCommand(
            other.unwrap_or("[none]").to_string(),
          )),
        })
      })
      .collect()
  }

  pub async fn run<C>(&self, context: &mut C) -> Result<(), C::Error>
  where
    C: Context,
  {
    match self {
      Self::Ping => context.reply("pong!".to_string()).await,
      Self::Merge => unimplemented!(),
    }
  }
}
