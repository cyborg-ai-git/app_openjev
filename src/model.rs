use std::collections::BTreeMap;

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// API primitives. Structured instructions and criteria are accepted as JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        instructions: Value,
        criteria: BTreeMap<String, Value>,
    },
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
    Noul {
        instructions: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<BTreeMap<String, Value>>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub state: Value,
    pub model: String,
    pub questions: BTreeMap<String, Question>,
}

fn content(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => false,
    }
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            content(&self.state),
            "State must contain nonempty text, an object, or an array"
        );
        ensure!(!self.model.trim().is_empty(), "Missing model");
        ensure!(!self.questions.is_empty(), "Provide at least one question");
        for (id, question) in &self.questions {
            ensure!(!id.trim().is_empty(), "Empty question identifier");
            let instructions = match question {
                Question::Choice {
                    instructions,
                    criteria,
                } => {
                    ensure!(
                        (2..=255).contains(&criteria.len()),
                        "Choice requires 2 to 255 options"
                    );
                    ensure!(
                        criteria
                            .iter()
                            .all(|(k, v)| !k.trim().is_empty() && (v.is_null() || content(v))),
                        "Invalid Choice options"
                    );
                    instructions
                }
                Question::Score {
                    instructions,
                    criteria,
                } => {
                    ensure!(
                        (2..=10).contains(&criteria.len()),
                        "Score requires 2 to 10 levels"
                    );
                    ensure!(
                        criteria.iter().all(content),
                        "Empty or invalid Score levels"
                    );
                    instructions
                }
                Question::Noul {
                    instructions,
                    criteria,
                } => {
                    if let Some(criteria) = criteria {
                        ensure!(
                            criteria
                                .iter()
                                .all(|(k, v)| (k == "true" || k == "false") && content(v)),
                            "Noul criteria only accept true and false"
                        );
                    }
                    instructions
                }
            };
            ensure!(content(instructions), "Missing instructions for {id}");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Noul {
        noul: f64,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluation {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}

fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn distribution(values: &BTreeMap<String, f64>, confidence: f64) -> Result<()> {
    ensure!(probability(confidence), "Confidence out of range");
    ensure!(
        values.values().all(|v| probability(*v)),
        "Invalid probabilities"
    );
    ensure!(
        (values.values().sum::<f64>() - 1.0).abs() <= 0.01,
        "Probabilities do not sum to 1"
    );
    Ok(())
}

impl Evaluation {
    /// Validate server answers against the actual submitted questions.
    pub fn validate_for(&self, request: &Request) -> Result<()> {
        ensure!(
            self.answers.keys().eq(request.questions.keys()),
            "Answer IDs do not match question IDs"
        );
        for (id, question) in &request.questions {
            match (question, &self.answers[id]) {
                (Question::Noul { .. }, Answer::Noul { noul }) => {
                    ensure!(probability(*noul), "Noul probability out of range");
                }
                (
                    Question::Choice { criteria, .. },
                    Answer::Choice {
                        choice,
                        probabilities,
                        confidence,
                    },
                ) => {
                    distribution(probabilities, *confidence)?;
                    ensure!(
                        criteria.contains_key(choice) && criteria.keys().eq(probabilities.keys()),
                        "Returned options do not match requested options"
                    );
                }
                (
                    Question::Score { criteria, .. },
                    Answer::Score {
                        score,
                        legend,
                        probabilities,
                        confidence,
                    },
                ) => {
                    distribution(probabilities, *confidence)?;
                    let keys: BTreeMap<_, _> =
                        (0..criteria.len()).map(|i| (i.to_string(), ())).collect();
                    ensure!(
                        keys.keys().eq(probabilities.keys()) && keys.keys().eq(legend.keys()),
                        "Invalid Score response levels"
                    );
                    ensure!(
                        score.is_finite() && (0.0..=(criteria.len() - 1) as f64).contains(score),
                        "Score out of range"
                    );
                }
                _ => bail!("Wrong answer type for {id}"),
            }
        }
        Ok(())
    }
}
