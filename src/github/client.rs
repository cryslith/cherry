use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;

static TOKEN_LIFESPAN: Lazy<Duration> = Lazy::new(|| Duration::minutes(10));

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
  #[serde(with = "ts_seconds")]
  iat: DateTime<Utc>,
  #[serde(with = "ts_seconds")]
  exp: DateTime<Utc>,
  iss: String,
}

pub struct Client {
  app_id: String,
  private_key: EncodingKey,
}

impl Client {
  pub fn new(app_id: String, private_key: EncodingKey) -> Self {
    Self {
      app_id,
      private_key,
    }
  }

  fn generate_token(&self) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
      &Header::new(Algorithm::RS256),
      &Claims {
        iat: Utc::now(),
        exp: Utc::now() + *TOKEN_LIFESPAN,
        iss: self.app_id.clone(),
      },
      &self.private_key,
    )
  }
}
