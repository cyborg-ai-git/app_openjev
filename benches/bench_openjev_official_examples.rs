//! Bounded, explicitly opted-in prediction checks on frozen official examples.
#[path = "../tests/support/u_openjev_official_cases.rs"]
mod cases;

use anyhow::{Result, ensure};
use openjev::{
    Evaluation, UOpenjevCredentials,
    local::{LocalModel, device},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn stats(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    json!({"n":n,"mean_ms":sorted.iter().sum::<f64>() / n as f64,
        "median_ms":(sorted[(n-1)/2]+sorted[n/2])/2.0,"min_ms":sorted[0],"max_ms":sorted[n-1]})
}

fn outcome(
    case: &cases::Case,
    result: Result<Evaluation>,
    elapsed_ms: f64,
    first_byte_ms: Option<u128>,
) -> Result<(Value, Option<Evaluation>)> {
    match result {
        Ok(response) => Ok((
            json!({"status":"ok","elapsed_ms":elapsed_ms,
            "first_byte_ms":first_byte_ms,"reference_checks":cases::reference_checks(case,&response)?,
            "response":response}),
            Some(response),
        )),
        Err(error) => Ok((
            json!({"status":"error","elapsed_ms":elapsed_ms,"error":error.to_string()}),
            None,
        )),
    }
}

fn summary(rows: &[Value]) -> Value {
    let mut kinds: BTreeMap<String, [usize; 3]> = BTreeMap::new();
    let mut references: BTreeMap<String, BTreeMap<String, [usize; 2]>> = BTreeMap::new();
    let mut local_ms = Vec::new();
    let mut remote_ms = Vec::new();
    let mut unsupported = Vec::new();
    let mut unexpected_failures = Vec::new();
    for row in rows {
        if row["local"]["status"] == "ok" && row["remote"]["status"] == "ok" {
            local_ms.push(row["local"]["elapsed_ms"].as_f64().unwrap());
            remote_ms.push(row["remote"]["elapsed_ms"].as_f64().unwrap());
        }
        if row["remote"]["status"] != "ok"
            || (row["local_supported"] == true && row["local"]["status"] != "ok")
        {
            unexpected_failures.push(json!({"case_id":row["case_id"],"repeat":row["repeat"]}));
        }
        if row["repeat"] != 0 {
            continue;
        }
        if row["local"]["status"] != "ok" {
            unsupported.push(row["case_id"].clone());
        }
        for item in row["comparisons"].as_array().unwrap() {
            let counts = kinds
                .entry(item["type"].as_str().unwrap().into())
                .or_default();
            counts[0] += 1;
            counts[1] += usize::from(item["decision_agrees"] == true);
            counts[2] += usize::from(item["exact_answer_agrees"] == true);
        }
        for backend in ["local", "remote"] {
            if let Some(checks) = row[backend]["reference_checks"].as_array() {
                for item in checks {
                    let counts = references
                        .entry(backend.into())
                        .or_default()
                        .entry(item["basis"].as_str().unwrap().into())
                        .or_default();
                    counts[0] += 1;
                    counts[1] += usize::from(item["matches_reference_decision"] == true);
                }
            }
        }
    }
    json!({"decision_counts_first_repeat_only":kinds,
        "decision_count_columns":["compared","same_decision","exact_answer"],
        "reference_counts_first_repeat_only":references,"reference_count_columns":["available_labels","matching_decisions"],
        "local_unavailable_cases":unsupported,"unexpected_failures":unexpected_failures,
        "latency_common_successful_pairs":{"local":stats(&local_ms),"remote":stats(&remote_ms),
            "remote_to_local_mean_ratio":if local_ms.is_empty(){None}else{Some(remote_ms.iter().sum::<f64>() / local_ms.iter().sum::<f64>())}}})
}

fn main() -> Result<()> {
    if std::env::var("OPENJEV_OFFICIAL_LIVE").as_deref() != Ok("1") {
        eprintln!(
            "Skipped official examples. OPENJEV_OFFICIAL_LIVE=1 opts into 43 API evaluations: one warmup and two passes over 21 frozen cases. Normal client retries may add HTTP attempts."
        );
        return Ok(());
    }
    let data = cases::dataset()?;
    ensure!(
        data.schema_version == 1 && data.cases.len() == 21,
        "Unexpected fixture version/count"
    );
    for case in &data.cases {
        case.request.validate()?;
        ensure!(
            case.source_ids
                .iter()
                .all(|id| data.sources.contains_key(id)),
            "Missing provenance"
        );
    }
    let config = std::env::var("OPENJEV_CONFIG")
        .unwrap_or_else(|_| "./config/.secrets/secret_env.toml".into());
    let credentials = UOpenjevCredentials::from_path_config(&config)
        .map_err(|_| anyhow::anyhow!("Could not load the Evo credential configuration"))?;
    let client = credentials.client("https://api.typesafe.ai", Duration::from_secs(30))?;
    drop(credentials);
    let directory = std::env::var("OPENJEV_MODEL_DIR")?;
    let name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let output = std::env::var("OPENJEV_OFFICIAL_OUTPUT")
        .unwrap_or_else(|_| "docs/measurements/official-examples.json".into());
    if let Some(parent) = Path::new(&output)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let model = LocalModel::load(Path::new(&directory), device(&name)?)?;
    for _ in 0..3 {
        model.evaluate(&data.cases[0].request)?;
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let start = Instant::now();
    runtime.block_on(client.evaluate(&data.cases[0].request))?;
    let remote_warmup_ms = start.elapsed().as_secs_f64() * 1000.0;
    let dataset: Value = serde_json::from_str(cases::FIXTURE)?;
    let fixture_sha256 = format!("{:x}", Sha256::digest(cases::FIXTURE.as_bytes()));
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let write_report = |rows: &[Value], complete: bool| -> Result<()> {
        let report = json!({"complete":complete,"unix_timestamp":timestamp,"device":name,
            "fixture_sha256":fixture_sha256,"dataset":dataset,"remote_warmup_ms_excluded":remote_warmup_ms,
            "method":"Two serial passes over each frozen request; alternate backend order; keep model resident and reuse HTTP client. No response cache, training or prompt tuning. Per-question agreement uses Choice labels, tied Score modes, and a 0.5 Noul threshold. Published example outputs are illustrative references, not ground truth. Eight fruit labels are independently assigned. SDK examples without semantic expectations count only toward backend agreement. Unsupported local inputs remain unchanged and are reported, not flattened or counted as prediction errors. Timing means use only successful matched pairs; repeated decisions are not counted as additional unique cases.",
            "summary":summary(rows),"pairs":rows});
        std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
        Ok(())
    };
    let mut rows = Vec::new();
    for repeat in 0..2 {
        for (index, case) in data.cases.iter().enumerate() {
            let local = || {
                let start = Instant::now();
                let response = model.evaluate(&case.request);
                outcome(case, response, start.elapsed().as_secs_f64() * 1000.0, None)
            };
            let remote = || {
                let start = Instant::now();
                let response = runtime.block_on(client.evaluate_timed(&case.request));
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                match response {
                    Ok((r, t)) => outcome(case, Ok(r), elapsed, t.first_byte_ms),
                    Err(e) => outcome(case, Err(e), elapsed, None),
                }
            };
            let local_first = (repeat * data.cases.len() + index) % 2 == 0;
            let ((local_json, local_response), (remote_json, remote_response)) = if local_first {
                let a = local()?;
                (a, remote()?)
            } else {
                let b = remote()?;
                (local()?, b)
            };
            let comparisons = match (&local_response, &remote_response) {
                (Some(a), Some(b)) => cases::compare(case, a, b)?,
                _ => Vec::new(),
            };
            let count = comparisons
                .iter()
                .filter(|c| c["decision_agrees"] == true)
                .count();
            println!(
                "Pass {} {}: local {}; remote {}; decisions {count}/{}",
                repeat + 1,
                case.id,
                local_json["status"],
                remote_json["status"],
                comparisons.len()
            );
            let unauthorized = remote_json["error"]
                .as_str()
                .is_some_and(|s| s.contains("HTTP 401") || s.contains("HTTP 403"));
            rows.push(json!({"case_id":case.id,"repeat":repeat,"source_ids":case.source_ids,"notes":case.notes,
                "local_supported":case.local_supported,"local_first":local_first,"local":local_json,"remote":remote_json,"comparisons":comparisons}));
            write_report(&rows, false)?;
            ensure!(
                !unauthorized,
                "Official API authorization failed; partial report saved"
            );
        }
    }
    write_report(&rows, true)?;
    ensure!(
        summary(&rows)["unexpected_failures"]
            .as_array()
            .unwrap()
            .is_empty(),
        "Unexpected backend failures; see report"
    );
    println!("Report written to {output}");
    Ok(())
}
