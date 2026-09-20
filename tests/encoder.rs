#![cfg(feature = "local")]
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::modernbert::{Config, ModernBert};
use openjev::local::encoder::Encoder;
use std::collections::HashMap;

fn weights(cfg: &Config, device: &Device) -> HashMap<String, Tensor> {
    let mut tensors = HashMap::new();
    let h = cfg.hidden_size;
    let mut insert = |name: String, shape: Vec<usize>, norm: bool| {
        let size = shape.iter().product();
        let values: Vec<f32> = (0..size)
            .map(|i| {
                if norm {
                    1.0
                } else {
                    ((i as f32 + name.len() as f32) * 0.17).sin() * 0.03
                }
            })
            .collect();
        tensors.insert(
            name,
            Tensor::from_vec(values, shape.as_slice(), device).unwrap(),
        );
    };
    insert(
        "model.embeddings.tok_embeddings.weight".into(),
        vec![cfg.vocab_size, h],
        false,
    );
    insert("model.embeddings.norm.weight".into(), vec![h], true);
    insert("model.final_norm.weight".into(), vec![h], true);
    for i in 0..cfg.num_hidden_layers {
        let prefix = format!("model.layers.{i}");
        if i > 0 {
            insert(format!("{prefix}.attn_norm.weight"), vec![h], true);
        }
        insert(format!("{prefix}.mlp_norm.weight"), vec![h], true);
        insert(format!("{prefix}.attn.Wqkv.weight"), vec![h * 3, h], false);
        insert(format!("{prefix}.attn.Wo.weight"), vec![h, h], false);
        insert(
            format!("{prefix}.mlp.Wi.weight"),
            vec![cfg.intermediate_size * 2, h],
            false,
        );
        insert(
            format!("{prefix}.mlp.Wo.weight"),
            vec![h, cfg.intermediate_size],
            false,
        );
    }
    tensors
}

#[test]
fn native_encoder_matches_candle_reference_with_local_and_global_attention() {
    let cfg = Config {
        vocab_size: 40,
        hidden_size: 64,
        num_hidden_layers: 4,
        num_attention_heads: 2,
        intermediate_size: 80,
        max_position_embeddings: 32,
        layer_norm_eps: 1e-5,
        pad_token_id: 0,
        global_attn_every_n_layers: 2,
        global_rope_theta: 160000.0,
        local_attention: 8,
        local_rope_theta: 10000.0,
        classifier_config: None,
    };
    let device = Device::Cpu;
    let vb = VarBuilder::from_tensors(weights(&cfg, &device), DType::F32, &device);
    let native = Encoder::load(vb.clone(), &cfg, 32).unwrap();
    let reference = ModernBert::load(vb, &cfg).unwrap();
    for length in [9, 17, 32] {
        let ids = Tensor::from_vec(
            (0..length).map(|i| (i % 39 + 1) as u32).collect(),
            (1, length),
            &device,
        )
        .unwrap();
        let mask = Tensor::ones((1, length), DType::U32, &device).unwrap();
        let a = native.forward(&ids).unwrap();
        let b = reference.forward(&ids, &mask).unwrap();
        let error = (a - b)
            .unwrap()
            .abs()
            .unwrap()
            .max_all()
            .unwrap()
            .to_scalar::<f32>()
            .unwrap();
        assert!(error < 1e-4, "encoder error {error} for length {length}");
    }
}
