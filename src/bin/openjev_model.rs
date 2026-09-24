//! Download data-only Laya checkpoints, pin their revision, and verify file hashes.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(about = "Download and verify original Laya weights; never executes remote code")]
struct Args {
    #[arg(long, default_value = "convaiinnovations/laya", value_parser = ["convaiinnovations/laya", "convaiinnovations/laya-multilingual", "convaiinnovations/laya-typed-decisions"])]
    repo: String,
    #[arg(long, default_value = "main")]
    revision: String,
    #[arg(long)]
    output: PathBuf,
}

fn digest(path: &std::path::Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut sha = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(format!("{:x}", sha.finalize()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.revision
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
        "Invalid revision"
    );
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(900))
        .build()?;
    let metadata: Value = http
        .get(format!(
            "https://huggingface.co/api/models/{}/revision/{}?blobs=true",
            args.repo, args.revision
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let revision = metadata["sha"].as_str().context("Missing model revision")?;
    ensure!(
        revision.len() == 40 && revision.chars().all(|c| c.is_ascii_hexdigit()),
        "Unexpected revision format"
    );
    let siblings = metadata["siblings"]
        .as_array()
        .context("Missing model file metadata")?;
    let files = [
        "rl_agent_config.json",
        "encoder/config.json",
        "tokenizer/tokenizer.json",
        "tokenizer/tokenizer_config.json",
        "model.safetensors",
    ];
    fs::create_dir_all(&args.output)?;
    let mut manifest = Vec::new();
    eprintln!("Resolved {} to {revision}", args.repo);
    for name in files {
        let meta = siblings
            .iter()
            .find(|v| v["rfilename"] == name)
            .with_context(|| format!("Missing checkpoint file: {name}"))?;
        let expected_hash = meta["lfs"]["sha256"].as_str();
        if name == "model.safetensors" {
            ensure!(
                expected_hash.is_some(),
                "Missing published SHA-256 for model weights"
            );
        }
        let destination = args.output.join(name);
        if destination.exists() {
            let actual = digest(&destination)?;
            let expected = if let Some(hash) = expected_hash {
                hash.to_owned()
            } else {
                // Git-backed configuration files have no LFS SHA-256. Compare
                // against the small file served at the immutable revision.
                let bytes = http
                    .get(format!(
                        "https://huggingface.co/{}/resolve/{revision}/{name}",
                        args.repo
                    ))
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;
                format!("{:x}", Sha256::digest(&bytes))
            };
            ensure!(
                actual == expected,
                "{} already exists and cannot be verified; choose a fresh output directory",
                destination.display()
            );
        } else {
            fs::create_dir_all(destination.parent().context("Invalid destination")?)?;
            let partial = destination.with_extension("partial");
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&partial)?;
            let download: Result<()> = async {
                eprintln!("Downloading {name}...");
                let mut response = http
                    .get(format!(
                        "https://huggingface.co/{}/resolve/{revision}/{name}",
                        args.repo
                    ))
                    .send()
                    .await?
                    .error_for_status()?;
                let mut sha = Sha256::new();
                let mut size = 0_u64;
                while let Some(chunk) = response.chunk().await? {
                    file.write_all(&chunk)?;
                    sha.update(&chunk);
                    size += chunk.len() as u64;
                }
                if let Some(expected_size) = meta["size"].as_u64() {
                    ensure!(size == expected_size, "Size mismatch for {name}");
                }
                if let Some(expected_hash) = expected_hash {
                    ensure!(
                        format!("{:x}", sha.finalize()) == expected_hash,
                        "SHA-256 mismatch for {name}"
                    );
                }
                file.sync_all()?;
                Ok(())
            }
            .await;
            drop(file);
            if let Err(error) = download {
                let _ = fs::remove_file(&partial);
                return Err(error);
            }
            fs::rename(partial, &destination)?;
        }
        manifest.push(json!({"file": name, "sha256": digest(&destination)?, "bytes": fs::metadata(&destination)?.len()}));
    }
    let manifest = json!({"repository": args.repo, "revision": revision, "license": "Apache-2.0 (upstream checkpoint)", "files": manifest});
    fs::write(
        args.output.join("openjev-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("Verified checkpoint: {}", args.output.display());
    Ok(())
}
