//! Task-level evaluation metrics. Probability calibration requires labeled held-out data.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub probabilities: Vec<f64>,
    pub label: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub count: usize,
    pub accuracy: f64,
    /// Multiclass Brier score: mean sum of squared errors across all classes.
    pub brier: f64,
    /// Mean negative log likelihood, with a numerical floor of 1e-12.
    pub nll: f64,
    /// Equal-width top-label expected calibration error.
    pub ece: f64,
}

pub fn evaluate(observations: &[Observation], bins: usize) -> Result<Metrics> {
    ensure!(
        !observations.is_empty() && (1..=1000).contains(&bins),
        "Provide observations and between 1 and 1000 calibration bins"
    );
    let mut buckets = vec![(0_usize, 0.0_f64, 0.0_f64); bins];
    let mut total_correct = 0.0;
    let mut brier = 0.0;
    let mut nll = 0.0;
    for row in observations {
        let p = &row.probabilities;
        ensure!(
            p.len() >= 2 && row.label < p.len(),
            "Invalid label or class count"
        );
        ensure!(
            p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && (p.iter().sum::<f64>() - 1.0).abs() < 0.001,
            "Invalid probability distribution"
        );
        let mut best = 0;
        for i in 1..p.len() {
            if p[i] > p[best] {
                best = i;
            }
        }
        let correct = f64::from(best == row.label);
        total_correct += correct;
        brier += p
            .iter()
            .enumerate()
            .map(|(i, value)| (value - f64::from(i == row.label)).powi(2))
            .sum::<f64>();
        nll -= p[row.label].max(1e-12).ln();
        let bucket = ((p[best] * bins as f64) as usize).min(bins - 1);
        buckets[bucket].0 += 1;
        buckets[bucket].1 += p[best];
        buckets[bucket].2 += correct;
    }
    let count = observations.len();
    let denominator = count as f64;
    let ece = buckets
        .iter()
        .map(|(_, confidence_sum, correct_sum)| (confidence_sum - correct_sum).abs() / denominator)
        .sum();
    Ok(Metrics {
        count,
        accuracy: total_correct / denominator,
        brier: brier / denominator,
        nll: nll / denominator,
        ece,
    })
}
