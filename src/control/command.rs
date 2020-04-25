use thiserror::Error;

#[derive(Debug)]
struct CommandComment {
  commands: Vec<Command>,
}

#[derive(Debug, Error)]
pub enum ParseError {
  #[error("unknown command: {0}")]
  UnknownCommand(String),
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
          other => Err(ParseError::UnknownCommand(other.unwrap_or("[none]").to_string())),
        })
      })
      .collect()
  }
}
