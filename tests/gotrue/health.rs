use gotrue::api::Client;

#[tokio::test]
async fn gotrue_health() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, "http://localhost:9998");
  gotrue_client.health().await.unwrap();
}
