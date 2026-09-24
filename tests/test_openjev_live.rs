use std::time::{Duration, Instant};

use openjev::{Request, UOpenjevCredentials};

#[test]
#[ignore = "Makes one paid official API request; requires OPENJEV_CONFIG or the default Evo secret file"]
fn official_api_accepts_evo_credentials_and_all_primitives() -> anyhow::Result<()> {
    let path = std::env::var("OPENJEV_CONFIG")
        .unwrap_or_else(|_| "./config/.secrets/secret_env.toml".into());
    let credentials = UOpenjevCredentials::from_path_config(&path)
        .map_err(|_| anyhow::anyhow!("Could not load the Evo credential configuration"))?;
    let client = credentials.client("https://api.typesafe.ai", Duration::from_secs(30))?;
    let mut request: Request = serde_json::from_str(include_str!("../examples/triage.json"))?;
    request.model = "jev-1.13.0".into();
    let start = Instant::now();
    let response = tokio::runtime::Runtime::new()?.block_on(client.evaluate(&request))?;
    response.validate_for(&request)?;
    assert_eq!(response.answers.len(), 3);
    // No credentials, request data, response content, or generated dataset are printed.
    println!(
        "Official Jev API verified: 3 primitives, {} ms, {} input tokens",
        start.elapsed().as_millis(),
        response.usage.input_tokens
    );
    Ok(())
}
