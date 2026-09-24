//! ModernBERT inference with reusable rotary tables and fused Metal attention.
//! Architecture: Answer.AI ModernBERT; tensor conventions follow Candle.
use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{
    Embedding, LayerNorm, Linear, Module, VarBuilder, embedding, layer_norm_no_bias, linear_no_bias,
};
use candle_transformers::models::modernbert::Config;

// Finite masks avoid inf - inf in tiled softmax and all-masked padding queries.
// Two combined masks remain representable in FP16. For normalized checkpoint
// activations, masked probabilities underflow to zero in both FP16 and FP32.
pub const MASK_BIAS: f32 = -10_000.0;

pub fn attention(q: &Tensor, k: &Tensor, v: &Tensor, mask: Option<&Tensor>) -> Result<Tensor> {
    let width = q.dim(3)?;
    let scale = 1.0 / (width as f64).sqrt();
    if q.device().is_metal() && q.dim(2)? > 8 && matches!(width, 32 | 64 | 72 | 80 | 96 | 128 | 256)
    {
        let expanded_mask = mask
            .map(|m| m.broadcast_as((q.dim(0)?, q.dim(1)?, q.dim(2)?, k.dim(2)?)))
            .transpose()?;
        return candle_nn::ops::sdpa(q, k, v, expanded_mask.as_ref(), false, scale as f32, 1.0);
    }
    let mut scores = (q.matmul(&k.transpose(2, 3)?)? * scale)?;
    if let Some(mask) = mask {
        scores = scores.broadcast_add(mask)?;
    }
    candle_nn::ops::softmax_last_dim(&scores)?.matmul(v)
}

struct Rope {
    cosine: Tensor,
    sine: Tensor,
}
impl Rope {
    fn new(width: usize, length: usize, theta: f64, dtype: DType, device: &Device) -> Result<Self> {
        // Compute angles in f32 before casting: half-precision position rounding loses accuracy.
        let frequencies: Vec<f32> = (0..width)
            .step_by(2)
            .map(|i| theta.powf(-(i as f64) / width as f64) as f32)
            .collect();
        let positions = Tensor::arange(0_u32, length as u32, device)?
            .to_dtype(DType::F32)?
            .reshape((length, 1))?;
        let angles =
            positions.matmul(&Tensor::new(frequencies.as_slice(), device)?.unsqueeze(0)?)?;
        Ok(Self {
            cosine: angles.cos()?.to_dtype(dtype)?,
            sine: angles.sin()?.to_dtype(dtype)?,
        })
    }
    fn apply(&self, tensor: &Tensor) -> Result<Tensor> {
        candle_nn::rotary_emb::rope(tensor, &self.cosine, &self.sine)
    }
}

struct Layer {
    qkv: Linear,
    out: Linear,
    attention_norm: Option<LayerNorm>,
    mlp_norm: LayerNorm,
    mlp_in: Linear,
    mlp_out: Linear,
    heads: usize,
    global: bool,
}

impl Layer {
    fn load(vb: VarBuilder<'_>, cfg: &Config, index: usize) -> Result<Self> {
        let hidden = cfg.hidden_size;
        Ok(Self {
            qkv: linear_no_bias(hidden, hidden * 3, vb.pp("attn.Wqkv"))?,
            out: linear_no_bias(hidden, hidden, vb.pp("attn.Wo"))?,
            attention_norm: if index == 0 {
                None
            } else {
                Some(layer_norm_no_bias(
                    hidden,
                    cfg.layer_norm_eps,
                    vb.pp("attn_norm"),
                )?)
            },
            mlp_norm: layer_norm_no_bias(hidden, cfg.layer_norm_eps, vb.pp("mlp_norm"))?,
            mlp_in: linear_no_bias(hidden, cfg.intermediate_size * 2, vb.pp("mlp.Wi"))?,
            mlp_out: linear_no_bias(cfg.intermediate_size, hidden, vb.pp("mlp.Wo"))?,
            heads: cfg.num_attention_heads,
            global: index.is_multiple_of(cfg.global_attn_every_n_layers),
        })
    }

    fn forward(&self, x: &Tensor, rope: &Rope, mask: Option<&Tensor>) -> Result<Tensor> {
        let (batch, length, hidden) = x.dims3()?;
        let normed = match &self.attention_norm {
            Some(norm) => norm.forward(x)?,
            None => x.clone(),
        };
        // Reshape before slicing: reshaping each strided Q/K/V slice would copy it.
        let projections = self.qkv.forward(&normed)?.reshape((
            batch,
            length,
            3,
            self.heads,
            hidden / self.heads,
        ))?;
        let projection = |index| {
            projections
                .narrow(2, index, 1)?
                .squeeze(2)?
                .transpose(1, 2)?
                .contiguous()
        };
        let q = rope.apply(&projection(0)?)?;
        let k = rope.apply(&projection(1)?)?;
        let v = projection(2)?;
        let attended = attention(&q, &k, &v, mask)?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((batch, length, hidden))?;
        let x = (x + self.out.forward(&attended)?)?;
        let parts = self
            .mlp_in
            .forward(&self.mlp_norm.forward(&x)?)?
            .chunk(2, 2)?;
        let gated = (parts[0].gelu_erf()? * &parts[1])?;
        &x + self.mlp_out.forward(&gated)?
    }
}

pub struct Encoder {
    embeddings: Embedding,
    embedding_norm: LayerNorm,
    layers: Vec<Layer>,
    final_norm: LayerNorm,
    global_rope: Rope,
    local_rope: Rope,
    local_mask: Tensor,
}

impl Encoder {
    pub fn load(vb: VarBuilder<'_>, cfg: &Config, max_len: usize) -> Result<Self> {
        let hidden = cfg.hidden_size;
        let width = hidden / cfg.num_attention_heads;
        let mask: Vec<_> = (0..max_len)
            .flat_map(|i| {
                (0..max_len).map(move |j| {
                    if i.abs_diff(j) <= cfg.local_attention / 2 {
                        0.0_f32
                    } else {
                        MASK_BIAS
                    }
                })
            })
            .collect();
        Ok(Self {
            embeddings: embedding(
                cfg.vocab_size,
                hidden,
                vb.pp("model.embeddings.tok_embeddings"),
            )?,
            embedding_norm: layer_norm_no_bias(
                hidden,
                cfg.layer_norm_eps,
                vb.pp("model.embeddings.norm"),
            )?,
            layers: (0..cfg.num_hidden_layers)
                .map(|i| Layer::load(vb.pp(format!("model.layers.{i}")), cfg, i))
                .collect::<Result<_>>()?,
            final_norm: layer_norm_no_bias(hidden, cfg.layer_norm_eps, vb.pp("model.final_norm"))?,
            global_rope: Rope::new(
                width,
                max_len,
                cfg.global_rope_theta,
                vb.dtype(),
                vb.device(),
            )?,
            local_rope: Rope::new(
                width,
                max_len,
                cfg.local_rope_theta,
                vb.dtype(),
                vb.device(),
            )?,
            local_mask: Tensor::from_vec(mask, (max_len, max_len), vb.device())?
                .to_dtype(vb.dtype())?,
        })
    }

    pub fn forward(&self, ids: &Tensor) -> Result<Tensor> {
        self.forward_masked(ids, None)
    }

    /// Right-padded batches use an additive key mask of shape (batch, 1, 1, length).
    /// Each row retains its own rotary positions and never attends to another row.
    pub fn forward_masked(&self, ids: &Tensor, padding: Option<&Tensor>) -> Result<Tensor> {
        let length = ids.dim(1)?;
        let mut mask = self
            .local_mask
            .narrow(0, 0, length)?
            .narrow(1, 0, length)?
            .contiguous()?
            .reshape((1, 1, length, length))?;
        if let Some(padding) = padding {
            mask = mask.broadcast_add(padding)?;
        }
        let mut x = self
            .embedding_norm
            .forward(&self.embeddings.forward(ids)?)?;
        for layer in &self.layers {
            x = layer.forward(
                &x,
                if layer.global {
                    &self.global_rope
                } else {
                    &self.local_rope
                },
                if layer.global { padding } else { Some(&mask) },
            )?;
        }
        self.final_norm.forward(&x)
    }
}
