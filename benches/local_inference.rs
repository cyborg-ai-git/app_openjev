use criterion::{Criterion, criterion_group, criterion_main};
use openjev::{
    Request,
    local::{LocalModel, device, softmax},
};
use std::{hint::black_box, path::Path, time::Duration};

fn benchmarks(c: &mut Criterion) {
    c.bench_function("local/softmax_255", |b| {
        b.iter(|| softmax(black_box(&[0.3; 255]), 1.2).unwrap())
    });
    let Ok(directory) = std::env::var("OPENJEV_MODEL_DIR") else {
        eprintln!("Inference not measured: set OPENJEV_MODEL_DIR and optionally OPENJEV_DEVICE");
        return;
    };
    let device_name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let model = LocalModel::load(Path::new(&directory), device(&device_name).unwrap()).unwrap();
    let request: Request = serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    let mut group = c.benchmark_group(format!("laya/{device_name}"));
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(10));
    group.bench_function("three_questions", |b| {
        b.iter(|| model.evaluate(black_box(&request)).unwrap())
    });
    group.finish();
}
criterion_group!(benches, benchmarks);
criterion_main!(benches);
