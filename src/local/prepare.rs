use anyhow::{Context, Result, ensure};
use serde_json::Value;
use tokenizers::Tokenizer;

use crate::Question;

pub struct Prepared {
    pub ids: Vec<u32>,
    pub markers: Vec<u32>,
    pub kind: usize,
}

fn text(value: &Value) -> Result<&str> {
    value
        .as_str()
        .context("The local Laya backend requires plain-text instructions and descriptions")
}

pub fn options(question: &Question) -> Result<(usize, &str, Vec<String>)> {
    Ok(match question {
        Question::Choice {
            instructions,
            criteria,
        } => (
            0,
            text(instructions)?,
            criteria
                .iter()
                .map(|(k, v)| {
                    if v.is_null() {
                        Ok(k.clone())
                    } else {
                        Ok(format!("{k}: {}", text(v)?))
                    }
                })
                .collect::<Result<_>>()?,
        ),
        Question::Score {
            instructions,
            criteria,
        } => (
            1,
            text(instructions)?,
            criteria
                .iter()
                .enumerate()
                .map(|(i, v)| Ok(format!("level {i}: {}", text(v)?)))
                .collect::<Result<_>>()?,
        ),
        Question::Noul {
            instructions,
            criteria,
        } => {
            let description = |key: &str, default: &str| -> Result<String> {
                Ok(criteria
                    .as_ref()
                    .and_then(|c| c.get(key))
                    .map(text)
                    .transpose()?
                    .unwrap_or(default)
                    .into())
            };
            (
                2,
                text(instructions)?,
                vec![
                    format!(
                        "false: {}",
                        description("false", "no, the statement does not hold")?
                    ),
                    format!("true: {}", description("true", "yes, the statement holds")?),
                ],
            )
        }
    })
}

/// Local input builder: markers are inserted as IDs, never taken from user text.
/// Unlike upstream, overflow is rejected instead of silently truncating the state.
pub fn prepare(
    tok: &Tokenizer,
    state: &Value,
    question: &Question,
    max_len: usize,
    head_max_len: usize,
    specials: [u32; 3],
) -> Result<Prepared> {
    let [cls, sep, mask] = specials;
    let mask_text = tok.id_to_token(mask).context("Missing MASK token")?;
    let encode = |s: &str| -> Result<Vec<u32>> {
        let sanitized = s.replace(&mask_text, " ");
        Ok(tok
            .encode(sanitized, false)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .get_ids()
            .to_vec())
    };
    let (kind, instruction, options) = options(question)?;
    let kind_name = ["choice", "score", "noul"][kind];
    let head = encode(&format!("{kind_name} question: {instruction}"))?;
    let encoded = options
        .iter()
        .map(|s| encode(&format!(" {s}")))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        encoded.iter().all(|v| v.len() <= 48),
        "An option exceeds 48 tokens: shorten it for Laya"
    );
    let options_len: usize = encoded.iter().map(|v| v.len() + 1).sum();
    ensure!(
        head.len() + options_len <= head_max_len,
        "Question and options exceed {head_max_len} tokens: shorten them (no silent truncation)"
    );
    let mut ids = vec![cls];
    ids.extend(head);
    ids.push(sep);
    let mut markers = Vec::with_capacity(encoded.len());
    for option in encoded {
        markers.push(ids.len() as u32);
        ids.push(mask);
        ids.extend(option);
    }
    ids.push(sep);
    let serialized;
    let state_text = if let Some(s) = state.as_str() {
        s
    } else {
        serialized = serde_json::to_string(state)?;
        &serialized
    };
    ids.extend(encode(state_text)?);
    ids.push(sep);
    ensure!(
        ids.len() <= max_len,
        "Local input: {} tokens, limit {max_len}. Shorten the state or criteria",
        ids.len()
    );
    Ok(Prepared { ids, markers, kind })
}
