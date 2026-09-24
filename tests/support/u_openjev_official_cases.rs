//! Frozen public examples and comparison rules shared by tests and opt-in benches.
use std::collections::BTreeMap;

use anyhow::Result;
use openjev::{Answer, Evaluation, Request};
use serde::Deserialize;
use serde_json::{Value, json};

pub const FIXTURE: &str = include_str!("../fixtures/official_examples.json");

#[derive(Deserialize)]
pub struct Dataset {
    pub schema_version: u32,
    pub sources: BTreeMap<String, Value>,
    pub cases: Vec<Case>,
}

#[derive(Deserialize)]
pub struct Case {
    pub id: String,
    pub source_ids: Vec<String>,
    pub notes: String,
    pub local_supported: bool,
    pub request: Request,
    pub expected: BTreeMap<String, Expected>,
}

#[derive(Deserialize)]
pub struct Expected {
    pub decision: Value,
    pub basis: String,
    pub published_value: Option<f64>,
}

pub fn dataset() -> Result<Dataset> {
    Ok(serde_json::from_str(FIXTURE)?)
}

/// Compare nominal decisions, retaining uncertainty and all tied Score modes.
pub fn decision(answer: &Answer) -> Value {
    match answer {
        Answer::Choice { choice, .. } => json!(choice),
        Answer::Score { probabilities, .. } => {
            let max = probabilities
                .values()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            json!(
                probabilities
                    .iter()
                    .filter(|(_, v)| (max - **v).abs() < 1e-12)
                    .map(|(k, _)| k)
                    .collect::<Vec<_>>()
            )
        }
        Answer::Noul { noul } => json!(if *noul > 0.5 {
            "yes"
        } else if *noul < 0.5 {
            "no"
        } else {
            "uncertain"
        }),
    }
}

fn probabilities(answer: &Answer) -> Vec<f64> {
    match answer {
        Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
            probabilities.values().copied().collect()
        }
        Answer::Noul { noul } => vec![1.0 - noul, *noul],
    }
}

fn scalar(answer: &Answer) -> Option<f64> {
    match answer {
        Answer::Noul { noul } => Some(*noul),
        Answer::Score { score, .. } => Some(*score),
        _ => None,
    }
}

pub fn reference_checks(case: &Case, response: &Evaluation) -> Result<Vec<Value>> {
    response.validate_for(&case.request)?;
    Ok(case.expected.iter().map(|(id, expected)| {
        let answer = &response.answers[id];
        let actual = decision(answer);
        json!({"question_id":id, "basis":expected.basis,
            "expected_decision":expected.decision, "observed_decision":actual,
            "matches_reference_decision":actual == expected.decision,
            "published_value":expected.published_value,
            "absolute_delta_from_published_value":scalar(answer).zip(expected.published_value).map(|(a,b)| (a-b).abs())})
    }).collect())
}

pub fn compare(case: &Case, local: &Evaluation, remote: &Evaluation) -> Result<Vec<Value>> {
    local.validate_for(&case.request)?;
    remote.validate_for(&case.request)?;
    case.request
        .questions
        .keys()
        .map(|id| {
            let a = &local.answers[id];
            let b = &remote.answers[id];
            let deltas: Vec<f64> = probabilities(a)
                .iter()
                .zip(probabilities(b))
                .map(|(a, b)| (a - b).abs())
                .collect();
            let kind = match a {
                Answer::Choice { .. } => "choice",
                Answer::Score { .. } => "score",
                Answer::Noul { .. } => "noul",
            };
            Ok(json!({"question_id":id, "type":kind,
            "local_decision":decision(a), "remote_decision":decision(b),
            "decision_agrees":decision(a) == decision(b),
            "exact_answer_agrees":serde_json::to_value(a)? == serde_json::to_value(b)?,
            "max_probability_delta":deltas.iter().copied().fold(0.0_f64,f64::max),
            "total_variation_distance":deltas.iter().sum::<f64>() / 2.0,
            "absolute_scalar_delta":scalar(a).zip(scalar(b)).map(|(a,b)| (a-b).abs())}))
        })
        .collect()
}
