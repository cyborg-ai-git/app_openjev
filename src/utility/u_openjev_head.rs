//! Candle implementation of the published Laya decision head (Apache-2.0).
//! See NOTICE for the original authors and source architecture.
use candle_core::{Result, Tensor};
use candle_nn::{LayerNorm, Linear, Module, VarBuilder, layer_norm, linear};

pub struct Layer {
    qkv: Linear,
    out: Linear,
    norm1: LayerNorm,
    norm2: LayerNorm,
    up: Linear,
    down: Linear,
    heads: usize,
}

impl Layer {
    pub fn load(vb: VarBuilder<'_>, hidden: usize) -> Result<Self> {
        Ok(Self {
            qkv: Linear::new(
                vb.get((hidden * 3, hidden), "self_attn.in_proj_weight")?,
                Some(vb.get(hidden * 3, "self_attn.in_proj_bias")?),
            ),
            out: linear(hidden, hidden, vb.pp("self_attn.out_proj"))?,
            norm1: layer_norm(hidden, 1e-5, vb.pp("norm1"))?,
            norm2: layer_norm(hidden, 1e-5, vb.pp("norm2"))?,
            up: linear(hidden, hidden * 4, vb.pp("linear1"))?,
            down: linear(hidden * 4, hidden, vb.pp("linear2"))?,
            heads: (hidden / 64).max(1),
        })
    }

    pub fn forward(&self, input: &Tensor) -> Result<Tensor> {
        self.forward_masked(input, None)
    }

    pub fn forward_masked(&self, input: &Tensor, padding: Option<&Tensor>) -> Result<Tensor> {
        let (batch, length, hidden) = input.dims3()?;
        let width = hidden / self.heads;
        let projected = self
            .qkv
            .forward(&self.norm1.forward(input)?)?
            .reshape((batch, length, 3, self.heads, width))?;
        let projection = |index| {
            projected
                .narrow(2, index, 1)?
                .squeeze(2)?
                .transpose(1, 2)?
                .contiguous()
        };
        let q = projection(0)?;
        let k = projection(1)?;
        let v = projection(2)?;
        let attention = super::encoder::attention(&q, &k, &v, padding)?;
        let attention = attention
            .transpose(1, 2)?
            .contiguous()?
            .reshape((batch, length, hidden))?;
        let x = (input + self.out.forward(&attention)?)?;
        let ff = self
            .down
            .forward(&self.up.forward(&self.norm2.forward(&x)?)?.relu()?)?;
        x + ff
    }
}

pub struct Scorer {
    norm: LayerNorm,
    hidden: Linear,
    output: Linear,
}

impl Scorer {
    pub fn load(vb: VarBuilder<'_>, hidden: usize) -> Result<Self> {
        Ok(Self {
            norm: layer_norm(hidden, 1e-5, vb.pp("0"))?,
            hidden: linear(hidden, hidden, vb.pp("1"))?,
            output: linear(hidden, 1, vb.pp("3"))?,
        })
    }

    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        self.output
            .forward(&self.hidden.forward(&self.norm.forward(x)?)?.gelu_erf()?)?
            .squeeze(2)
    }
}
