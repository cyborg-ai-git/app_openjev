//! Read a JSON array of {probabilities, label} and print task-level quality metrics.
use anyhow::{Context, Result};
use openjev::metrics::{Observation, evaluate};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("Usage: cargo run --example metrics -- observations.json")?;
    let rows: Vec<Observation> = serde_json::from_slice(&std::fs::read(path)?)?;
    println!("{}", serde_json::to_string_pretty(&evaluate(&rows, 15)?)?);
    Ok(())
}
