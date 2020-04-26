use std::collections::HashMap;
use std::fmt;

use actix_web::client::{Client as AwcClient, ClientRequest, ClientResponse, PayloadError};
use actix_web::http::{header, uri, Method, StatusCode};
use actix_web::web::Bytes;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, Utc};
use futures::prelude::Stream;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

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

#[derive(Debug, Serialize)]
struct Claims {
  #[serde(with = "ts_seconds")]
  iat: DateTime<Utc>,
  #[serde(with = "ts_seconds")]
  exp: DateTime<Utc>,
  iss: String,
}

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

pub struct Client {
  credentials: Credentials,
  app_token_cache: Option<Token>,
  installation_tokens: HashMap<Repository, Token>,
  client: AwcClient,
}

impl Client {
  // FIXME need to share credential cache across requests
  pub fn new(credentials: Credentials, client: AwcClient) -> Self {
    Self {
      credentials,
      app_token_cache: None,
      installation_tokens: HashMap::new(),
      client,
    }
  }

  fn generate_app_token(&self) -> Result<Token, ClientError> {
    let iat = Utc::now();
    let exp = iat + Duration::seconds(APP_TOKEN_LIFESPAN_SECS);
    Ok(Token {
      token: encode(
        &Header::new(Algorithm::RS256),
        &Claims {
          iat,
          exp,
          iss: self.credentials.app_id.clone(),
        },
        &self.credentials.private_key,
      )?,
      renew: exp - Duration::seconds(APP_TOKEN_RENEW_AHEAD_SECS),
    })
  }

  fn app_token(&mut self) -> Result<&Token, ClientError> {
    let token = match self.app_token_cache.take() {
      Some(token) if Utc::now() < token.renew => token,
      _ => self.generate_app_token()?,
    };
    Ok(self.app_token_cache.get_or_insert(token))
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
      let mut response = self
        .app_request(Method::GET, uri)?
        .send()
        .await
        .map_err(|e| ClientError::SendRequest(e))?;
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
        .app_request(Method::POST, uri)?
        .send_json(&TokenRequest {
          repository_ids: vec![repo.id],
          permissions: [(PermissionType::Issues, Permission::Write)]
            .iter()
            .copied()
            .collect(),
        })
        .await
        .map_err(|e| ClientError::SendRequest(e))?;
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

  async fn repo_token(&mut self, repo: Repository) -> Result<&Token, ClientError> {
    let token = match self.installation_tokens.remove(&repo) {
      Some(token) if Utc::now() < token.renew => token,
      _ => self.request_repo_token(&repo).await?,
    };
    Ok(self.installation_tokens.entry(repo).or_insert(token))
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

  pub fn app_request(
    &mut self,
    method: Method,
    uri: uri::Uri,
  ) -> Result<ClientRequest, ClientError> {
    Ok(self.api_request(method, uri).set_header(
      header::AUTHORIZATION,
      format!("Bearer {}", self.app_token()?.token),
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
