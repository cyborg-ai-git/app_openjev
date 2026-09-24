use openjev::{
    EnumOpenjevBackend,
    app::{App, Completed, demo_response},
    ui,
};
use ratatui::{Terminal, backend::TestBackend, style::Color};

fn rgb(color: Color) -> [u8; 3] {
    match color {
        Color::Rgb(r, g, b) => [r, g, b],
        Color::White => [255, 255, 255],
        Color::LightRed => [255, 100, 100],
        Color::Yellow => [240, 205, 95],
        _ => [226, 234, 245],
    }
}

#[test]
#[ignore = "Writes synthetic TestBackend visual fixtures to OPENJEV_VISUAL_DIR; no real credentials or clipboard"]
fn export_visual_fixtures() {
    let directory = std::path::PathBuf::from(std::env::var("OPENJEV_VISUAL_DIR").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    let mut app = App::new(
        "jev-1.13.0".into(),
        "I was charged twice for the same order. Please refund the extra payment today.",
        true,
        None,
        ".".into(),
    );
    let request = app.request().unwrap();
    app.last = Some(Completed {
        backend: EnumOpenjevBackend::Demo,
        ttft_ms: None,
        first_byte_ms: None,
        demo: true,
        elapsed_ms: 350,
        response: demo_response(&request),
        request,
    });
    for (name, width, height, settings) in [
        ("wide", 120, 36, false),
        ("compact", 70, 24, false),
        ("settings", 90, 28, true),
    ] {
        if settings {
            app.open_settings();
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        let cells: Vec<_> = terminal.backend().buffer().content.iter().map(|cell|
            serde_json::json!({"text": cell.symbol(), "fg": rgb(cell.fg), "bg": rgb(cell.bg)})).collect();
        let value = serde_json::json!({"width": width, "height": height, "cells": cells});
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
}
