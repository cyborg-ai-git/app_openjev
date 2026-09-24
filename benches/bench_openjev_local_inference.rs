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
    if std::env::var_os("OPENJEV_BENCH_COMPARE").is_some() {
        let mut single = request.clone();
        single.questions.retain(|id, _| id == "refund");
        let mut long = request.clone();
        long.state = serde_json::json!(format!(
            "{} {}",
            "The customer has an active account and previous orders. ".repeat(24),
            request.state.as_str().unwrap()
        ));
        let mut many = request.clone();
        many.questions = (0..4)
            .flat_map(|i| {
                request
                    .questions
                    .iter()
                    .map(move |(id, q)| (format!("{i}_{id}"), q.clone()))
            })
            .collect();
        for (name, workload) in [
            ("single_question", &single),
            ("long_three_questions", &long),
            ("twelve_questions", &many),
        ] {
            group.bench_function(name, |b| {
                b.iter(|| model.evaluate(black_box(workload)).unwrap())
            });
        }
        for (name, workload) in [
            ("sequential_three_questions", &request),
            ("sequential_single_question", &single),
            ("sequential_long_three_questions", &long),
            ("sequential_twelve_questions", &many),
        ] {
            group.bench_function(name, |b| {
                b.iter(|| model.evaluate_sequential(black_box(workload)).unwrap())
            });
        }
    }
    group.finish();
}
criterion_group!(benches, benchmarks);
criterion_main!(benches);
