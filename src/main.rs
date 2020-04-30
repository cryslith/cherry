use cherry::github::client::{Credentials, TokenCache};
use cherry::github::webhook::webhook;

use std::env;
use std::error::Error as _;
use std::io;
use std::sync::Arc;

use actix_web::{middleware::Logger, web, App, HttpServer};
use clap::{crate_authors, crate_description, crate_name, crate_version, AppSettings, SubCommand};
use jsonwebtoken::EncodingKey;
use log::info;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
enum MainError {
  #[error("binding address")]
  Bind(#[source] io::Error),
  #[error("running server")]
  Run(#[source] io::Error),
  #[error("loading environment variable `{0}`")]
  Env(String, #[source] env::VarError),
  #[error("base64-decoding private key")]
  Base64(#[from] base64::DecodeError),
  #[error("parsing private key")]
  PrivateKey(#[from] jsonwebtoken::errors::Error),
  #[error("database error")]
  DB(#[from] quaint::error::Error),
  #[cfg(migration)]
  #[error("migrating database")]
  Migration(#[from] cherry::db::MigrationError),
}

fn var(v: &str) -> Result<String, MainError> {
  env::var(v).map_err(|e| MainError::Env(v.to_string(), e))
}

#[actix_rt::main]
async fn main() {
  std::process::exit(match main_().await {
    Ok(_) => 0,
    Err(e) => {
      eprintln!("error: {}", e);
      let mut e = e.source();
      while let Some(c) = e {
        eprintln!("caused by: {}", c);
        e = c.source();
      }
      1
    }
  });
}

async fn main_() -> Result<(), MainError> {
  drop(dotenv::dotenv());
  env_logger::init();

  let app = clap::App::new(crate_name!())
    .version(crate_version!())
    .author(crate_authors!())
    .about(crate_description!())
    .subcommand(SubCommand::with_name("run").about("run the server"))
    .setting(AppSettings::SubcommandRequired);

  #[cfg(migration)]
  app.subcommand(SubCommand::with_name("migrate").about("run database migrtions"));

  let matches = app.get_matches();

  if let Some(_) = matches.subcommand_matches("run") {
    return run().await;
  }
  #[cfg(migration)]
  {
    if let Some(_) = matches.subcommand_matches("migrate") {
      return migrate().await;
    }
  }
  panic!("invalid subcommand");
}

#[cfg(migration)]
async fn migrate() -> Result<(), MainError> {
  use barrel::SqlVariant;
  use quaint::connector::ConnectionInfo;

  let db_addr = var("DATABASE_ADDRESS")?;
  let db = quaint::single::Quaint::new(db_addr.as_str()).await?;
  let db_type = match db.connection_info() {
    ConnectionInfo::Sqlite { .. } => SqlVariant::Sqlite,
  };
  cherry::db::migrate(&db, db_type, cherry::db::migrations().as_slice()).await?;
  Ok(())
}

async fn run() -> Result<(), MainError> {
  let credentials = {
    let private_key = var("GITHUB_APP_PRIVATE_KEY")?;
    let private_key = base64::decode(private_key)?;
    let private_key = EncodingKey::from_rsa_pem(&private_key[..])?;
    let app_id = var("GITHUB_APP_ID")?;
    Credentials {
      app_id,
      private_key,
    }
  };

  let token_cache = Arc::new(Mutex::new(TokenCache::new()));

  let bind_address = env::var("BIND_ADDRESS").unwrap_or("127.0.0.1:8080".to_string());

  info!("listening on {}", bind_address);
  HttpServer::new(move || {
    App::new()
      .data(credentials.clone())
      .data(token_cache.clone())
      .wrap(Logger::default())
      .route("/webhook", web::post().to(webhook))
  })
  .bind(bind_address)
  .map_err(MainError::Bind)?
  .run()
  .await
  .map_err(MainError::Run)
}
