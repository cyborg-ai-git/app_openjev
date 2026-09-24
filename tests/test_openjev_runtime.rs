use std::process::Command;

use openjev::{Evaluation, Request, app::demo_response};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method},
};

fn request_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/triage.json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn executable_loads_default_evo_file_and_ignores_legacy_key() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = directory.path().join("config/.secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    std::fs::write(
        secrets.join("secret_env.toml"),
        "[TYPESAFE_TOKEN]\nenabled = true\nvalue = 'synthetic-runtime-token'\n",
    )
    .unwrap();
    let server = MockServer::start().await;
    let request: Request = serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    Mock::given(method("POST"))
        .and(header("Authorization", "Bearer synthetic-runtime-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(demo_response(&request)))
        .expect(1)
        .mount(&server)
        .await;
    let output = Command::new(env!("CARGO_BIN_EXE_openjev"))
        .current_dir(directory.path())
        .env("TYPESAFE_API_KEY", "unused-legacy-value")
        .args(["--base-url", &server.uri(), "--request"])
        .arg(request_path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let evaluation: Evaluation = serde_json::from_slice(&output.stdout).unwrap();
    evaluation.validate_for(&request).unwrap();
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("synthetic-runtime-token"));
        assert!(!text.contains("unused-legacy-value"));
    }
}

#[test]
fn executable_rejects_invalid_config_without_disclosing_parser_input() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("secret_env.toml");
    std::fs::write(
        &config,
        "[TYPESAFE_TOKEN]\nvalue = 'synthetic-runtime-token'\n[broken",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_openjev"))
        .args(["--config"])
        .arg(config)
        .arg("--request")
        .arg(request_path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Could not load enabled TYPESAFE_TOKEN"));
    assert!(!error.contains("synthetic-runtime-token"));
}

#[test]
fn demo_does_not_require_secret_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_openjev"))
        .current_dir(directory.path())
        .args(["--demo", "--request"])
        .arg(request_path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(serde_json::from_slice::<Evaluation>(&output.stdout).is_ok());
}
