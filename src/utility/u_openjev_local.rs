//! Native, offline inference for original Laya ModernBERT checkpoints.
//! This is an independent runtime, not the proprietary Jev model.
#[path = "u_openjev_encoder.rs"]
pub mod encoder;
#[path = "u_openjev_head.rs"]
mod head;
#[path = "u_openjev_prepare.rs"]
pub mod prepare;

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use candle_core::{DType, Device, Tensor};
use candle_nn::{Embedding, Module, VarBuilder, embedding};
use candle_transformers::models::modernbert::{Config, ModernBert};
use serde::Deserialize;
use serde_json::{Value, json};
use tokenizers::Tokenizer;

use crate::{Answer, Evaluation, Question, Request, Usage};

#[derive(Clone, Debug, Deserialize)]
pub struct AgentConfig {
    pub head_layers: usize,
    pub max_len: usize,
    pub head_max_len: usize,
    #[serde(default = "default_temperatures")]
    pub temperature: [f64; 3],
    #[serde(default)]
    pub temperature_by_options: BTreeMap<String, f64>,
}
fn default_temperatures() -> [f64; 3] {
    [1.0; 3]
}

#[derive(Clone, Copy)]
pub struct LoadOptions {
    pub dtype: DType,
    pub reference_encoder: bool,
}

enum Backbone {
    Native(encoder::Encoder),
    Reference(ModernBert),
}

/// Native inference without Python or an external inference server.
pub struct UOpenjevLocalModel {
    encoder: Backbone,
    head: Vec<head::Layer>,
    type_embedding: Embedding,
    scorer: head::Scorer,
    tokenizer: Tokenizer,
    config: AgentConfig,
    device: Device,
    specials: [u32; 3],
    name: String,
}
pub type LocalModel = UOpenjevLocalModel;

pub fn device(name: &str) -> Result<Device> {
    match name {
        "cpu" => Ok(Device::Cpu),
        "metal" => {
            #[cfg(feature = "metal")]
            {
                ensure!(
                    !candle_metal_kernels::metal::Device::all().is_empty(),
                    "No Metal device is accessible; use --device cpu or run outside a GPU-restricted sandbox"
                );
                Ok(Device::new_metal(0)?)
            }
            #[cfg(not(feature = "metal"))]
            {
                anyhow::bail!("Rebuild with --features metal")
            }
        }
        "cuda" => {
            #[cfg(feature = "cuda")]
            {
                Ok(Device::new_cuda(0)?)
            }
            #[cfg(not(feature = "cuda"))]
            {
                anyhow::bail!("Rebuild with --features cuda")
            }
        }
        _ => anyhow::bail!("Unknown device: choose cpu, metal, or cuda"),
    }
}

impl LocalModel {
    pub fn load(directory: &Path, device: Device) -> Result<Self> {
        let dtype = if device.is_metal() {
            DType::F16
        } else {
            DType::F32
        };
        Self::load_with_options(
            directory,
            device,
            LoadOptions {
                dtype,
                reference_encoder: false,
            },
        )
    }

    pub fn load_with_options(
        directory: &Path,
        device: Device,
        options: LoadOptions,
    ) -> Result<Self> {
        ensure!(
            matches!(options.dtype, DType::F32 | DType::F16),
            "Only f32 and f16 precision are supported"
        );
        ensure!(
            !options.reference_encoder || options.dtype == DType::F32,
            "The reference encoder requires f32"
        );
        let config: AgentConfig =
            serde_json::from_slice(&std::fs::read(directory.join("rl_agent_config.json"))?)?;
        ensure!(
            config.max_len >= 8 && config.head_max_len >= 8 && config.head_max_len < config.max_len,
            "Invalid checkpoint limits"
        );
        ensure!(
            config
                .temperature
                .iter()
                .chain(config.temperature_by_options.values())
                .all(|t| t.is_finite() && *t > 0.0),
            "Invalid temperatures"
        );
        let mut encoder_json: Value =
            serde_json::from_slice(&std::fs::read(directory.join("encoder/config.json"))?)?;
        ensure!(
            encoder_json["model_type"] == "modernbert",
            "This runtime only supports Laya ModernBERT checkpoints"
        );
        for flag in ["norm_bias", "attention_bias", "mlp_bias"] {
            ensure!(
                !encoder_json[flag].as_bool().unwrap_or(false),
                "Encoder variant with {flag} is not supported"
            );
        }
        ensure!(
            encoder_json["hidden_activation"].as_str().unwrap_or("gelu") == "gelu",
            "Unsupported encoder activation"
        );
        for (field, kind, default) in [
            ("global_rope_theta", "full_attention", 160000.0),
            ("local_rope_theta", "sliding_attention", 10000.0),
        ] {
            let rope = &encoder_json["rope_parameters"][kind];
            ensure!(
                rope["rope_type"].as_str().unwrap_or("default") == "default",
                "Scaled RoPE is not supported"
            );
            let value = encoder_json[field]
                .as_f64()
                .or_else(|| rope["rope_theta"].as_f64())
                .unwrap_or(default);
            encoder_json[field] = json!(value);
        }
        let encoder_config: Config = serde_json::from_value(encoder_json.clone())?;
        ensure!(
            config.max_len <= encoder_config.max_position_embeddings,
            "Context exceeds encoder capacity"
        );
        let hidden = encoder_config.hidden_size;
        ensure!(
            hidden > 0
                && encoder_config.num_attention_heads > 0
                && hidden.is_multiple_of(encoder_config.num_attention_heads)
                && (hidden / encoder_config.num_attention_heads).is_multiple_of(2)
                && hidden.is_multiple_of((hidden / 64).max(1))
                && encoder_config.global_attn_every_n_layers > 0,
            "Unsupported encoder dimensions"
        );
        if let Some(layers) = encoder_json["layer_types"].as_array() {
            ensure!(
                layers.len() == encoder_config.num_hidden_layers
                    && layers.iter().enumerate().all(|(i, v)| v
                        == if i % encoder_config.global_attn_every_n_layers == 0 {
                            "full_attention"
                        } else {
                            "sliding_attention"
                        }),
                "Unsupported layer_types pattern"
            );
        }
        let mut tokenizer = Tokenizer::from_file(directory.join("tokenizer/tokenizer.json"))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        tokenizer
            .with_truncation(None)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        tokenizer.with_padding(None);
        let tokenizer_config: Value = serde_json::from_slice(&std::fs::read(
            directory.join("tokenizer/tokenizer_config.json"),
        )?)?;
        let special = |name: &str| -> Result<u32> {
            let value = &tokenizer_config[name];
            let token = value
                .as_str()
                .or_else(|| value["content"].as_str())
                .with_context(|| format!("Missing {name}"))?;
            tokenizer
                .token_to_id(token)
                .with_context(|| format!("{name} is not in the vocabulary"))
        };
        let specials = [
            special("cls_token")?,
            special("sep_token")?,
            special("mask_token")?,
        ];
        let weights = candle_core::safetensors::load(directory.join("model.safetensors"), &device)?;
        let weights: HashMap<String, Tensor> = weights
            .into_iter()
            .map(|(key, tensor)| {
                let name = key
                    .strip_prefix("encoder.")
                    .map_or_else(|| key.clone(), |suffix| format!("model.{suffix}"));
                (name, tensor)
            })
            .collect();
        let vb = VarBuilder::from_tensors(weights, options.dtype, &device);
        let encoder = if options.reference_encoder {
            Backbone::Reference(ModernBert::load(vb.clone(), &encoder_config)?)
        } else {
            Backbone::Native(encoder::Encoder::load(
                vb.clone(),
                &encoder_config,
                config.max_len,
            )?)
        };
        let head = (0..config.head_layers)
            .map(|i| head::Layer::load(vb.pp(format!("head.layers.{i}")), hidden))
            .collect::<candle_core::Result<Vec<_>>>()?;
        let type_embedding = embedding(3, hidden, vb.pp("type_emb"))?;
        let scorer = head::Scorer::load(vb.pp("scorer"), hidden)?;
        Ok(Self {
            encoder,
            head,
            type_embedding,
            scorer,
            tokenizer,
            config,
            device,
            specials,
            name: format!(
                "local-laya/{}",
                directory.file_name().unwrap_or_default().to_string_lossy()
            ),
        })
    }

    /// Use bounded, independent-question batches on Metal; CPU/CUDA retain the serial path.
    pub fn evaluate(&self, request: &Request) -> Result<Evaluation> {
        self.evaluate_inner(request, self.device.is_metal())
    }

    /// Unbatched inference for numerical comparisons and an explicit compatibility fallback.
    pub fn evaluate_sequential(&self, request: &Request) -> Result<Evaluation> {
        self.evaluate_inner(request, false)
    }

    fn evaluate_inner(&self, request: &Request, batch: bool) -> Result<Evaluation> {
        request.validate()?;
        let mut answers = BTreeMap::new();
        // Preflight every question before GPU work, and preserve the public ID ordering.
        let mut inputs = request
            .questions
            .iter()
            .map(|(id, question)| {
                let input = prepare::prepare(
                    &self.tokenizer,
                    &request.state,
                    question,
                    self.config.max_len,
                    self.config.head_max_len,
                    self.specials,
                )?;
                Ok((id, question, input))
            })
            .collect::<Result<Vec<_>>>()?;
        let input_tokens = inputs.iter().map(|(_, _, p)| p.ids.len() as u64).sum();
        if batch {
            inputs.sort_by_key(|(_, _, p)| p.ids.len());
        }
        let mut start = 0;
        while start < inputs.len() {
            let mut end = start + 1;
            if batch {
                // Bound activations and padding overhead even for large question sets.
                while end < inputs.len()
                    && end - start < 4
                    && inputs[end].2.ids.len() <= 128
                    && inputs[end].2.ids.len() * 2 <= inputs[start].2.ids.len() * 3
                {
                    end += 1;
                }
            }
            let group = &inputs[start..end];
            let count = group.len();
            let length = group.iter().map(|(_, _, p)| p.ids.len()).max().unwrap();
            let mut ids = vec![0_u32; count * length];
            let mut mask = vec![encoder::MASK_BIAS; count * length];
            let mut kinds = Vec::with_capacity(count);
            let mut markers = Vec::new();
            for (row, (_, _, prepared)) in group.iter().enumerate() {
                let offset = row * length;
                ids[offset..offset + prepared.ids.len()].copy_from_slice(&prepared.ids);
                mask[offset..offset + prepared.ids.len()].fill(0.0);
                kinds.push(prepared.kind as u32);
                markers.extend(prepared.markers.iter().map(|m| offset as u32 + m));
            }
            let padding = if count > 1 && group.iter().any(|(_, _, p)| p.ids.len() < length) {
                Some(
                    Tensor::from_vec(mask, (count, 1, 1, length), &self.device)?
                        .to_dtype(self.type_embedding.embeddings().dtype())?,
                )
            } else {
                None
            };
            let ids = Tensor::from_vec(ids, (count, length), &self.device)?;
            let mut hidden = match &self.encoder {
                Backbone::Native(encoder) => encoder.forward_masked(&ids, padding.as_ref())?,
                Backbone::Reference(encoder) => {
                    let mask: Vec<u32> = group
                        .iter()
                        .flat_map(|(_, _, p)| (0..length).map(|i| u32::from(i < p.ids.len())))
                        .collect();
                    let mask = Tensor::from_vec(mask, (count, length), &self.device)?;
                    encoder.forward(&ids, &mask)?
                }
            };
            let kind = Tensor::new(kinds.as_slice(), &self.device)?;
            hidden = hidden.broadcast_add(&self.type_embedding.forward(&kind)?.unsqueeze(1)?)?;
            for layer in &self.head {
                hidden = if let Some(padding) = &padding {
                    layer.forward_masked(&hidden, Some(padding))?
                } else {
                    layer.forward(&hidden)?
                };
            }
            let markers = Tensor::new(markers.as_slice(), &self.device)?;
            let selected = hidden
                .flatten(0, 1)?
                .index_select(&markers, 0)?
                .unsqueeze(0)?;
            let logits = self
                .scorer
                .forward(&selected)?
                .squeeze(0)?
                .to_dtype(DType::F32)?
                .to_vec1::<f32>()?;
            let mut offset = 0;
            for (id, question, prepared) in group {
                let option_count = prepared.markers.len();
                let question_logits = &logits[offset..offset + option_count];
                offset += option_count;
                let bucket = temp_bucket(prepared.kind, option_count);
                let temperature = self
                    .config
                    .temperature_by_options
                    .get(&bucket)
                    .copied()
                    .unwrap_or(self.config.temperature[prepared.kind]);
                let probabilities = softmax(
                    &question_logits
                        .iter()
                        .map(|v| f64::from(*v))
                        .collect::<Vec<_>>(),
                    temperature,
                )?;
                answers.insert((*id).clone(), answer(question, &probabilities)?);
            }
            start = end;
        }
        let evaluation = Evaluation {
            model: self.name.clone(),
            answers,
            usage: Usage {
                input_tokens,
                output_tokens: 0,
            },
        };
        evaluation.validate_for(request)?;
        Ok(evaluation)
    }
}

pub fn temp_bucket(kind: usize, count: usize) -> String {
    let size = match count {
        0..=2 => "2",
        3..=5 => "3-5",
        6..=10 => "6-10",
        _ => "11+",
    };
    format!(
        "{}:{size}",
        ["choice", "score", "noul"].get(kind).unwrap_or(&"unknown")
    )
}

pub fn softmax(logits: &[f64], temperature: f64) -> Result<Vec<f64>> {
    ensure!(
        !logits.is_empty()
            && logits.iter().all(|v| v.is_finite())
            && temperature.is_finite()
            && temperature > 0.0,
        "Invalid logits or temperature"
    );
    let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let exp: Vec<_> = logits
        .iter()
        .map(|z| ((z - max) / temperature).exp())
        .collect();
    let sum: f64 = exp.iter().sum();
    Ok(exp.iter().map(|v| v / sum).collect())
}

/// Laya uses one minus normalized entropy; do not assume Jev uses the same formula.
pub fn confidence(p: &[f64]) -> f64 {
    if p.len() < 2 {
        return 1.0;
    }
    (1.0 + p
        .iter()
        .filter(|v| **v > 0.0)
        .map(|v| v * v.ln())
        .sum::<f64>()
        / (p.len() as f64).ln())
    .clamp(0.0, 1.0)
}

pub fn answer(question: &Question, p: &[f64]) -> Result<Answer> {
    let expected = match question {
        Question::Choice { criteria, .. } => criteria.len(),
        Question::Score { criteria, .. } => criteria.len(),
        Question::Noul { .. } => 2,
    };
    ensure!(
        p.len() == expected
            && p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && (p.iter().sum::<f64>() - 1.0).abs() < 0.001,
        "Invalid distribution"
    );
    Ok(match question {
        Question::Noul { .. } => Answer::Noul { noul: p[1] },
        Question::Choice { criteria, .. } => {
            let mut best = 0;
            for i in 1..p.len() {
                if p[i] > p[best] {
                    best = i;
                }
            }
            Answer::Choice {
                choice: criteria
                    .keys()
                    .nth(best)
                    .context("Choice has no options")?
                    .clone(),
                probabilities: criteria.keys().cloned().zip(p.iter().copied()).collect(),
                confidence: confidence(p),
            }
        }
        Question::Score { criteria, .. } => Answer::Score {
            score: p.iter().enumerate().map(|(i, p)| i as f64 * p).sum(),
            legend: criteria
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    (
                        i.to_string(),
                        v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                    )
                })
                .collect(),
            probabilities: p
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), *v))
                .collect(),
            confidence: confidence(p),
        },
    })
}
