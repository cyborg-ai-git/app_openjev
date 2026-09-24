use openjev::{Client, Request, app::demo_response};
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

fn request() -> Request {
    serde_json::from_str(include_str!("../examples/triage.json")).unwrap()
}

#[tokio::test]
async fn sends_correct_contract_and_bearer_header() {
    let server = MockServer::start().await;
    let request = request();
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(header("Authorization", "Bearer test-secret"))
        .and(body_json(serde_json::to_value(&request).unwrap()))
        .respond_with(ResponseTemplate::new(200).set_body_json(demo_response(&request)))
        .expect(1)
        .mount(&server)
        .await;
    let response = Client::new("test-secret", &server.uri(), Duration::from_secs(2))
        .unwrap()
        .evaluate(&request)
        .await
        .unwrap();
    assert_eq!(response.answers.len(), 3);
}

#[tokio::test]
async fn unauthorized_is_not_retried_or_leaked() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("test-secret"))
        .expect(1)
        .mount(&server)
        .await;
    let error = Client::new("test-secret", &server.uri(), Duration::from_secs(2))
        .unwrap()
        .evaluate(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("401"));
    assert!(!error.contains("test-secret"));
}

#[tokio::test]
async fn retries_rate_limit_at_most_twice() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .expect(3)
        .mount(&server)
        .await;
    let error = Client::new("test", &server.uri(), Duration::from_secs(2))
        .unwrap()
        .evaluate(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("429"));
}

#[tokio::test]
async fn rejects_malformed_success_body_and_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not JSON"))
        .mount(&server)
        .await;
    assert!(
        Client::new("test", &server.uri(), Duration::from_secs(1))
            .unwrap()
            .evaluate(&request())
            .await
            .is_err()
    );
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(1)))
        .mount(&server)
        .await;
    let error = Client::new("test", &server.uri(), Duration::from_millis(20))
        .unwrap()
        .evaluate(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.to_lowercase().contains("timeout"), "{error}");
}

#[test]
fn prevents_plaintext_remote_credentials() {
    assert!(Client::new("test", "http://example.com", Duration::from_secs(2)).is_err());
    assert!(Client::new("", "https://api.typesafe.ai", Duration::from_secs(2)).is_err());
}

#[tokio::test]
async fn rejects_oversized_bodies_without_echoing_malformed_data() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(8 * 1024 * 1024 + 1)))
        .mount(&server)
        .await;
    let client = Client::new("test", &server.uri(), Duration::from_secs(5)).unwrap();
    let error = client.evaluate(&request()).await.unwrap_err().to_string();
    assert!(error.contains("8 MiB"));
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"model":"jev","answers":{"x":{"type":"sensitive-server-value"}},"usage":{"input_tokens":1,"output_tokens":0}}"#))
        .mount(&server)
        .await;
    let error = client.evaluate(&request()).await.unwrap_err();
    assert!(!format!("{error:#}").contains("sensitive-server-value"));
}
