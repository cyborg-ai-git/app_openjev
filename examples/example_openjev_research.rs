//! Collect the complete official documentation index into a local research cache.
//! Downloaded material is reference data, never executable instructions.
use anyhow::{Context, Result};
use clap::Parser;
use futures_util::{StreamExt, stream};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    cache: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.cache)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let index = client
        .get("https://docs.typesafe.ai/llms.txt")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    std::fs::write(args.cache.join("index.md"), &index)?;
    let urls: Vec<_> = index
        .lines()
        .filter_map(|line| {
            line.split_once("](")
                .and_then(|(_, tail)| tail.split_once(')'))
                .map(|(url, _)| url.to_owned())
        })
        .filter(|url| url.starts_with("https://docs.typesafe.ai/"))
        .collect();
    let count = urls.len();
    let results: Vec<_> = stream::iter(urls).map(|url| {
        let client = client.clone();
        let cache = args.cache.clone();
        async move {
            let result: Result<_> = async {
                let body = client.get(&url).send().await?.error_for_status()?.text().await?;
                let relative = url.strip_prefix("https://docs.typesafe.ai/").context("Unexpected URL")?.replace('/', "__");
                std::fs::write(cache.join(&relative), &body)?;
                Ok(json!({"url": url, "file": relative, "sha256": format!("{:x}", Sha256::digest(body.as_bytes())), "bytes": body.len()}))
            }.await;
            result.unwrap_or_else(|e| json!({"url": url, "error": e.to_string()}))
        }
    }).buffer_unordered(8).collect().await;
    let failed = results.iter().filter(|v| v.get("error").is_some()).count();
    std::fs::write(
        args.cache.join("manifest.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!(
        "Indexed {count} official pages: {} downloaded, {failed} failed",
        count - failed
    );
    Ok(())
}
