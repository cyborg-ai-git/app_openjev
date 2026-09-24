//! Validate an Evo credential file and create a client without making an API call.
use std::time::Duration;

use openjev::UOpenjevCredentials;

fn main() -> anyhow::Result<()> {
    let credentials = UOpenjevCredentials::from_path_config("./config/.secrets/secret_env.toml")
        .map_err(|_| anyhow::anyhow!("Could not load the Evo credential configuration"))?;
    let _client = credentials.client("https://api.typesafe.ai", Duration::from_secs(30))?;
    println!("Evo credentials loaded; TypeSafe client ready. No API request sent.");
    Ok(())
}
