use actix_web::Responder;

pub async fn webhook() -> impl Responder {
  "hello"
}
