use criterion::{Criterion, criterion_group, criterion_main};
use openjev::{
    Request,
    app::{App, demo_response},
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use std::{hint::black_box, path::PathBuf};

fn benchmarks(c: &mut Criterion) {
    let request: Request = serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    c.bench_function("request/validate", |b| {
        b.iter(|| black_box(&request).validate().unwrap())
    });
    c.bench_function("request/serialize", |b| {
        b.iter(|| serde_json::to_vec(black_box(&request)).unwrap())
    });
    let result = demo_response(&request);
    c.bench_function("response/validate", |b| {
        b.iter(|| black_box(&result).validate_for(&request).unwrap())
    });
    let mut app = App::new(
        "demo".into(),
        "Example message",
        true,
        None,
        PathBuf::from("."),
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    c.bench_function("tui/render_120x36", |b| {
        b.iter(|| {
            terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        })
    });
}
criterion_group!(benches, benchmarks);
criterion_main!(benches);
