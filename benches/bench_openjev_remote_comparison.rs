//! Opt-in, bounded comparison of one public fixture against the official API.
//! Never use Criterion iteration counts for billed remote calls.
use std::{
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use openjev::{
    Evaluation, Request, UOpenjevCredentials,
    local::{LocalModel, device},
};
use serde_json::{Value, json};

fn summary(values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    json!({
        "samples": count,
        "mean_ms": sorted.iter().sum::<f64>() / count as f64,
        "median_ms": (sorted[(count - 1) / 2] + sorted[count / 2]) / 2.0,
        "min_ms": sorted[0],
        "max_ms": sorted[count - 1],
    })
}

fn local(model: &LocalModel, request: &Request) -> Result<(Evaluation, f64)> {
    let start = Instant::now();
    let response = model.evaluate(request)?;
    Ok((response, start.elapsed().as_secs_f64() * 1000.0))
}

fn main() -> Result<()> {
    if std::env::var("OPENJEV_COMPARE_LIVE").as_deref() != Ok("1") {
        eprintln!(
            "Skipped live comparison. Set OPENJEV_COMPARE_LIVE=1 to make 11 official API evaluations (one warmup and ten measured calls; rate-limit retries may add HTTP attempts)."
        );
        return Ok(());
    }
    let config = std::env::var("OPENJEV_CONFIG")
        .unwrap_or_else(|_| "./config/.secrets/secret_env.toml".into());
    // Evo configuration must be loaded before any runtime/tokenizer worker threads.
    let credentials = UOpenjevCredentials::from_path_config(&config)
        .map_err(|_| anyhow::anyhow!("Could not load the Evo credential configuration"))?;
    let client = credentials.client("https://api.typesafe.ai", Duration::from_secs(30))?;
    drop(credentials);
    let directory = std::env::var("OPENJEV_MODEL_DIR").context("Set OPENJEV_MODEL_DIR")?;
    let device_name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let output = std::env::var("OPENJEV_COMPARE_OUTPUT")
        .unwrap_or_else(|_| "docs/measurements/jev-vs-local.json".into());
    let mut request: Request = serde_json::from_str(include_str!("../examples/triage.json"))?;
    request.model = "jev-1.13.0".into();
    request.validate()?;
    let start = Instant::now();
    let model = LocalModel::load(Path::new(&directory), device(&device_name)?)?;
    let model_load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut warmup_ms = Vec::new();
    for _ in 0..3 {
        warmup_ms.push(local(&model, &request)?.1);
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let remote = || -> Result<(Evaluation, f64, Option<u128>)> {
        let start = Instant::now();
        let (response, timing) = runtime.block_on(client.evaluate_timed(&request))?;
        Ok((
            response,
            start.elapsed().as_secs_f64() * 1000.0,
            timing.first_byte_ms,
        ))
    };
    let (first_response, first_remote_ms, first_remote_byte_ms) = remote()?;
    let mut samples = Vec::new();
    let mut local_times = Vec::new();
    let mut remote_times = Vec::new();
    for index in 0..10 {
        let ((local_response, local_ms), (remote_response, remote_ms, first_byte_ms)) =
            if index % 2 == 0 {
                let local = local(&model, &request)?;
                (local, remote()?)
            } else {
                let remote = remote()?;
                (local(&model, &request)?, remote)
            };
        local_times.push(local_ms);
        remote_times.push(remote_ms);
        let exact_answers_match = serde_json::to_value(&local_response.answers)?
            == serde_json::to_value(&remote_response.answers)?;
        samples.push(json!({
            "index": index,
            "order": if index % 2 == 0 { "local_then_remote" } else { "remote_then_local" },
            "local_ms": local_ms,
            "remote_ms": remote_ms,
            "remote_first_byte_ms": first_byte_ms,
            "exact_answers_match": exact_answers_match,
            "local_response": local_response,
            "remote_response": remote_response,
        }));
        println!(
            "Pair {}: local {:.3} ms; official API {:.3} ms; exact answers match: {}",
            index + 1,
            local_ms,
            remote_ms,
            exact_answers_match
        );
    }
    let report = json!({
        "unix_timestamp": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        "purpose": "Latency and output comparison only; no training or distillation",
        "request": request,
        "device": device_name,
        "model_load_ms_excluded": model_load_ms,
        "local_warmup_ms_excluded": warmup_ms,
        "first_remote_call_excluded": { "total_ms": first_remote_ms,
            "first_byte_ms": first_remote_byte_ms, "response": first_response },
        "method": "Ten serial paired evaluations of the identical request; alternating backend order; resident local model; reused HTTP client; remote time includes network, HTTP retries if any, parsing and validation. Local time includes tokenization, inference, GPU readback and validation. No response cache. TTFT unavailable: neither backend streams generated tokens. No p95/p99 claim from ten observations.",
        "local": summary(&local_times),
        "remote": summary(&remote_times),
        "remote_to_local_mean_ratio": remote_times.iter().sum::<f64>() / local_times.iter().sum::<f64>(),
        "pairs": samples,
    });
    if let Some(parent) = Path::new(&output)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("Report written to {output}");
    Ok(())
}
