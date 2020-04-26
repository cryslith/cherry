use cherry::github::client::Credentials;
use cherry::github::webhook::webhook;

use std::env;
use std::io;

use actix_web::{middleware::Logger, web, App, HttpServer};
use jsonwebtoken::EncodingKey;
use log::info;

#[actix_rt::main]
async fn main() -> io::Result<()> {
  drop(dotenv::dotenv());
  env_logger::init();

  let credentials = {
    let private_key = env::var("GITHUB_APP_PRIVATE_KEY")
      .expect("failed to load private key from GITHUB_APP_PRIVATE_KEY");
    let private_key = base64::decode(private_key).expect("failed to base64-decode private key");
    let private_key = EncodingKey::from_rsa_pem(&private_key[..]).expect("invalid rsa private key");
    let app_id = env::var("GITHUB_APP_ID").expect("failed to load app id from GITHUB_APP_ID");
    Credentials {
      app_id,
      private_key,
    }
  };

  let bind_address = "127.0.0.1:8088";

  info!("listening on {}", bind_address);
  HttpServer::new(move || {
    App::new()
      .data(credentials.clone())
      .wrap(Logger::default())
      .route("/webhook", web::post().to(webhook))
  })
  .bind(bind_address)?
  .run()
  .await
}
