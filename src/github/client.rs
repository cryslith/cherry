use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use actix_web::client::{Client as AwcClient, ClientRequest, ClientResponse, PayloadError};
use actix_web::http::{header, uri, Method, StatusCode};
use actix_web::web::Bytes;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, Utc};
use futures::prelude::Stream;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

const APP_TOKEN_LIFESPAN_SECS: i64 = 10 * 60;
const APP_TOKEN_RENEW_AHEAD_SECS: i64 = 30;
const REPO_TOKEN_RENEW_AHEAD_SECS: i64 = 30;

#[derive(Debug, Deserialize)]
pub struct ServerError {
  message: String,
  errors: Option<Vec<ServerErrorDetail>>,
}

#[derive(Debug, Deserialize)]
pub struct ServerErrorDetail {
  resource: String,
  field: String,
  code: String,
  message: Option<String>,
  documentation_url: Option<String>,
}

#[derive(Debug, Error)]
pub enum ClientError {
  #[error("encoding json web token")]
  JWT(#[from] jsonwebtoken::errors::Error),
  #[error("in http library")]
  Http(#[from] actix_web::http::Error),
  #[error("sending request")]
  SendRequest(actix_web::client::SendRequestError),
  #[error("decoding json payload")]
  JsonPayload, // no re-export of awc::error::JsonPayloadError
  #[error("server returned error response")]
  ServerErrorResponse(StatusCode, Result<ServerError, String>),
}

impl From<actix_web::client::SendRequestError> for ClientError {
  fn from(e: actix_web::client::SendRequestError) -> Self {
    Self::SendRequest(e)
  }
}

#[derive(Debug, Serialize)]
struct Claims {
  #[serde(with = "ts_seconds")]
  iat: DateTime<Utc>,
  #[serde(with = "ts_seconds")]
  exp: DateTime<Utc>,
  iss: String,
}

#[derive(Clone)]
struct Token {
  token: String,
  renew: DateTime<Utc>,
}

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
struct Installation {
  id: i64,
}

#[allow(unused)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
enum PermissionType {
  Administration,
  Blocking,
  Checks,
  ContentReferences,
  Contents,
  Deployments,
  Emails,
  Followers,
  GpgKeys,
  Issues,
  Keys,
  Members,
  Metadata,
  OrganizationAdministration,
  OrganizationHooks,
  OrganizationPlan,
  OrganizationProjects,
  OrganizationUserBlocking,
  Pages,
  Plan,
  PullRequests,
  RepositoryHooks,
  RepositoryProjects,
  SingleFile,
  Starring,
  Statuses,
  TeamDiscussions,
  VulnerabilityAlerts,
  Watching,
}

#[allow(unused)]
#[derive(Debug, Copy, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum Permission {
  None,
  Read,
  Write,
  Admin,
}

#[derive(Debug, Serialize)]
struct TokenRequest {
  repository_ids: Vec<i64>,
  permissions: HashMap<PermissionType, Permission>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
  token: String,
  expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Credentials {
  pub app_id: String,
  pub private_key: EncodingKey,
}

impl Credentials {
  fn generate_app_token(&self) -> Result<Token, ClientError> {
    let iat = Utc::now();
    let exp = iat + Duration::seconds(APP_TOKEN_LIFESPAN_SECS);
    Ok(Token {
      token: encode(
        &Header::new(Algorithm::RS256),
        &Claims {
          iat,
          exp,
          iss: self.app_id.clone(),
        },
        &self.private_key,
      )?,
      renew: exp - Duration::seconds(APP_TOKEN_RENEW_AHEAD_SECS),
    })
  }
}

pub struct TokenCache {
  app_token: Option<Token>,
  installation_tokens: HashMap<Repository, Token>,
}

impl TokenCache {
  pub fn new() -> Self {
    Self {
      app_token: None,
      installation_tokens: HashMap::new(),
    }
  }

  fn app_token(&mut self, credentials: &Credentials) -> Result<Token, ClientError> {
    match &self.app_token {
      Some(token) if Utc::now() < token.renew => Ok(token.clone()),
      _ => {
        let token = credentials.generate_app_token()?;
        self.app_token = Some(token.clone());
        Ok(token)
      }
    }
  }
}

pub struct Client {
  credentials: Credentials,
  // TODO use a resource pool to avoid contending on the cache
  token_cache: Arc<Mutex<TokenCache>>,
  client: AwcClient,
}

impl Client {
  pub fn new(
    credentials: Credentials,
    token_cache: Arc<Mutex<TokenCache>>,
    client: AwcClient,
  ) -> Self {
    Self {
      credentials,
      token_cache,
      client,
    }
  }

  async fn app_token(&self) -> Result<Token, ClientError> {
    self.token_cache.lock().await.app_token(&self.credentials)
  }

  pub async fn response_ok<S>(response: &mut ClientResponse<S>) -> Result<(), ClientError>
  where
    S: Stream<Item = Result<Bytes, PayloadError>> + Unpin,
  {
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
      return Ok(());
    }
    Err(ClientError::ServerErrorResponse(
      status,
      match response.json().await {
        Ok(e) => Ok(e),
        Err(_) => Err(
          match response.body().await {
            Ok(ref body) => std::str::from_utf8(body).unwrap_or("error decoding error body"),
            Err(_) => "error getting error body",
          }
          .to_string(),
        ),
      },
    ))
  }

  async fn request_repo_token(&mut self, repo: &Repository) -> Result<Token, ClientError> {
    let installation: Installation = {
      let uri = self
        .api()
        .path_and_query(format!("/repos/{}/installation", repo).as_str())
        .build()?;
      let mut response = self.app_request(Method::GET, uri).await?.send().await?;
      Self::response_ok(&mut response).await?;
      response
        .json()
        .await
        .map_err(|_| ClientError::JsonPayload)?
    };
    let TokenResponse { token, expires_at } = {
      let uri = self
        .api()
        .path_and_query(format!("/app/installations/{}/access_tokens", installation.id).as_str())
        .build()?;
      let mut response = self
        .app_request(Method::POST, uri)
        .await?
        .send_json(&TokenRequest {
          repository_ids: vec![repo.id],
          permissions: [(PermissionType::Issues, Permission::Write)]
            .iter()
            .copied()
            .collect(),
        })
        .await?;
      Self::response_ok(&mut response).await?;
      response
        .json()
        .await
        .map_err(|_| ClientError::JsonPayload)?
    };
    Ok(Token {
      token,
      renew: expires_at - Duration::seconds(REPO_TOKEN_RENEW_AHEAD_SECS),
    })
  }

  async fn repo_token(&mut self, repo: Repository) -> Result<Token, ClientError> {
    let maybe_token = self
      .token_cache
      .lock()
      .await
      .installation_tokens
      .get(&repo)
      .cloned();
    match maybe_token {
      Some(token) if Utc::now() < token.renew => Ok(token),
      _ => {
        let token = self.request_repo_token(&repo).await?;
        self
          .token_cache
          .lock()
          .await
          .installation_tokens
          .insert(repo, token.clone());
        Ok(token)
      }
    }
  }

  pub fn api(&self) -> uri::Builder {
    uri::Builder::new()
      .scheme("https")
      .authority("api.github.com")
  }

  pub fn api_request(&self, method: Method, uri: uri::Uri) -> ClientRequest {
    self
      .client
      .request(method, uri)
      .set_header(
        header::ACCEPT,
        "application/vnd.github.machine-man-preview+json",
      )
      .set_header(header::USER_AGENT, "cryslith/cherry")
  }

  pub async fn app_request(
    &mut self,
    method: Method,
    uri: uri::Uri,
  ) -> Result<ClientRequest, ClientError> {
    Ok(self.api_request(method, uri).set_header(
      header::AUTHORIZATION,
      format!("Bearer {}", self.app_token().await?.token),
    ))
  }

  pub async fn repo_request(
    &mut self,
    repo: Repository,
    method: Method,
    uri: uri::Uri,
  ) -> Result<ClientRequest, ClientError> {
    Ok(self.api_request(method, uri).set_header(
      header::AUTHORIZATION,
      format!("Bearer {}", self.repo_token(repo).await?.token),
    ))
  }
}
