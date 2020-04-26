use thiserror::Error;
use async_trait::async_trait;

#[derive(Debug, Error)]
pub enum ParseError {
  #[error("unknown command: {0}")]
  UnknownCommand(String),
}

#[async_trait(?Send)]
pub trait Context {
  async fn reply(&mut self, message: String);
}

#[derive(Debug)]
pub enum Command {
  Ping,
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
          other => Err(ParseError::UnknownCommand(
            other.unwrap_or("[none]").to_string(),
          )),
        })
      })
      .collect()
  }

  pub async fn run(self, context: &mut impl Context) {
    match self {
      Self::Ping => {
        context.reply("pong!".to_string()).await;
      }
    }
  }
}
