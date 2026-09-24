use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use openjev::{
    Client, EnumOpenjevBackend as Backend,
    app::{App, EnumOpenjevAction as Action, demo_response, editor},
    ui,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

fn app() -> App {
    App::new(
        "jev-1.13.0".into(),
        "hello café 🦀 world",
        true,
        None,
        ".".into(),
    )
}
fn draw(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
fn region(app: &App, action: Action) -> Rect {
    app.mouse_regions
        .iter()
        .find(|(_, target)| *target == action)
        .unwrap()
        .0
}
fn event(rect: Rect, kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    }
}
fn click(app: &mut App, action: Action) -> bool {
    app.mouse(event(
        region(app, action),
        MouseEventKind::Down(MouseButton::Left),
    ))
}
async fn finish(app: &mut App) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while app.pending.is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
            app.collect().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn mouse_targets_follow_resize_and_help_blocks_underlying_controls() {
    let mut app = app();
    for (width, height) in [(120, 36), (80, 32), (70, 24)] {
        draw(&mut app, width, height);
        for (rect, _) in &app.mouse_regions {
            assert!(rect.right() <= width && rect.bottom() <= height);
        }
        click(&mut app, Action::Kind(2));
        assert_eq!(app.selected, 2);
        click(&mut app, Action::StateFormat);
        assert!(app.json_state);
        click(&mut app, Action::StateFormat);
        let editor_rect = region(&app, Action::Editor(0));
        click(&mut app, Action::Help);
        draw(&mut app, width, height);
        app.mouse(event(editor_rect, MouseEventKind::Down(MouseButton::Left)));
        assert!(app.help);
        click(&mut app, Action::Help);
        assert!(!app.help);
    }
    draw(&mut app, 30, 8);
    assert!(app.mouse_regions.is_empty());
    assert!(!app.mouse(event(
        Rect::new(1, 1, 1, 1),
        MouseEventKind::Down(MouseButton::Left)
    )));
}

#[test]
fn mouse_places_cursor_on_unicode_wrapped_and_scrolled_text() {
    let mut app = app();
    draw(&mut app, 120, 36);
    let mut target = region(&app, Action::Editor(0));
    target.x += 13; // Immediately after the two-column crab emoji.
    app.mouse(event(target, MouseEventKind::Down(MouseButton::Left)));
    assert_eq!(app.state.cursor(), (0, 12));
    app.key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(app.state.lines()[0], "hello café 🦀X world");
    app.state = editor(
        &(0..40)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    draw(&mut app, 120, 36);
    let rect = region(&app, Action::Editor(0));
    app.mouse(event(rect, MouseEventKind::ScrollDown));
    draw(&mut app, 120, 36);
    click(&mut app, Action::Editor(0));
    assert_eq!(app.state.cursor(), (3, 0));
    app.state = editor(&"word ".repeat(50));
    draw(&mut app, 70, 32);
    let mut rect = region(&app, Action::Editor(0));
    rect.y += 1;
    app.mouse(event(rect, MouseEventKind::Down(MouseButton::Left)));
    assert_eq!(app.state.screen_cursor().row, 1);
    assert_eq!(app.state.screen_cursor().col, 0);
    assert!(app.state.cursor().1 > 0);
}

#[tokio::test]
async fn mouse_evaluates_previews_scrolls_and_exports_timing_with_source() {
    let mut app = app();
    draw(&mut app, 120, 36);
    click(&mut app, Action::Evaluate);
    finish(&mut app).await;
    assert_eq!(app.last.as_ref().unwrap().backend, Backend::Demo);
    let visible = draw(&mut app, 120, 36);
    assert!(visible.contains("Response:") && visible.contains("TTFT: N/A"));
    click(&mut app, Action::Preview);
    assert!(app.raw);
    let rect = region(&app, Action::Results);
    app.mouse(event(rect, MouseEventKind::ScrollDown));
    assert_eq!(app.scroll, 3);
    app.mouse(event(rect, MouseEventKind::ScrollUp));
    assert_eq!(app.scroll, 0);
    let directory = tempfile::tempdir().unwrap();
    app.export_dir = directory.path().into();
    click(&mut app, Action::Export);
    let file = std::fs::read_dir(directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(saved["backend"], "demo");
    assert!(saved["ttft_ms"].is_null() && saved["first_byte_ms"].is_null());
    assert!(saved["elapsed_ms"].as_u64().unwrap() >= 300);
    assert!(click(&mut app, Action::Quit));
}

#[tokio::test]
async fn switching_cancels_old_result_preserves_inputs_and_never_falls_back() {
    let mut app = app();
    let state = app.state.lines().to_vec();
    app.submit();
    app.select_backend(Backend::Remote);
    assert!(app.pending.is_none());
    assert!(app.error);
    tokio::time::sleep(Duration::from_millis(400)).await;
    app.collect().await;
    assert!(app.last.is_none());
    app.submit();
    assert!(app.pending.is_none());
    assert_eq!(app.state.lines(), state);
    assert_eq!(app.model, "jev-1.13.0");
    app.select_backend(Backend::Demo);
    app.submit();
    finish(&mut app).await;
    app.select_backend(Backend::Remote);
    let text = draw(&mut app, 120, 36);
    assert!(text.contains("DEMO result"));
    assert_eq!(app.last.as_ref().unwrap().backend, Backend::Demo);
    app.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(app.backend, Backend::Local);
    app.submit();
    assert!(app.pending.is_none());
    app.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(app.backend, Backend::Remote);
}

#[tokio::test]
async fn remote_timing_measures_delayed_response_and_survives_export() {
    let server = MockServer::start().await;
    let mut app = app();
    let request = app.request().unwrap();
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(80))
                .set_body_json(demo_response(&request)),
        )
        .expect(1)
        .mount(&server)
        .await;
    app.client = Some(Client::new("fixture", &server.uri(), Duration::from_secs(3)).unwrap());
    app.select_backend(Backend::Remote);
    app.submit();
    finish(&mut app).await;
    let result = app.last.as_ref().unwrap();
    assert_eq!(result.backend, Backend::Remote);
    assert!(result.ttft_ms.is_none());
    let first_byte = result.first_byte_ms.unwrap();
    assert!(first_byte >= 60 && first_byte <= result.elapsed_ms);
    app.raw = true;
    let text = draw(&mut app, 120, 36);
    assert!(text.contains("TTFB:") && text.contains("TTFT: N/A"));
}

#[cfg(feature = "local")]
#[tokio::test]
async fn failed_background_load_does_not_interrupt_remote_and_can_be_retried() {
    let mut app = app();
    let directory = tempfile::tempdir().unwrap();
    app.local_config = Some(openjev::UOpenjevLocalConfig {
        directory: directory.path().into(),
        device: "cpu".into(),
        precision: "auto".into(),
    });
    app.select_backend(Backend::Local);
    assert!(app.local_loading.is_some());
    app.select_backend(Backend::Demo);
    let status = app.status.clone();
    tokio::time::timeout(Duration::from_secs(3), async {
        while app.local_loading.is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
            app.collect().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(app.status, status);
    assert!(!app.error && app.local_error.is_some());
    app.select_backend(Backend::Local);
    assert!(app.local_loading.is_some());
}

#[cfg(feature = "local")]
#[tokio::test]
#[ignore = "Requires OPENJEV_MODEL_DIR and real CPU/Metal access; remote uses a mock server"]
async fn real_local_model_stays_resident_across_remote_switches() {
    let mut app = app();
    app.local_config = Some(openjev::UOpenjevLocalConfig {
        directory: std::env::var("OPENJEV_MODEL_DIR").unwrap().into(),
        device: std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into()),
        precision: "auto".into(),
    });
    app.select_backend(Backend::Local);
    tokio::time::timeout(Duration::from_secs(30), async {
        while app.local_loading.is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
            app.collect().await;
        }
    })
    .await
    .unwrap();
    let resident = app.local.clone().expect("Model loaded");
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(demo_response(&app.request().unwrap())),
        )
        .expect(2)
        .mount(&server)
        .await;
    app.client = Some(Client::new("fixture", &server.uri(), Duration::from_secs(3)).unwrap());
    for backend in [
        Backend::Remote,
        Backend::Local,
        Backend::Remote,
        Backend::Local,
    ] {
        app.select_backend(backend);
        assert!(app.local_loading.is_none());
        assert!(std::sync::Arc::ptr_eq(
            &resident,
            app.local.as_ref().unwrap()
        ));
        app.submit();
        finish(&mut app).await;
        let result = app.last.as_ref().expect("Evaluation completed");
        assert_eq!(result.backend, backend);
        assert_eq!(result.request.model, "jev-1.13.0");
        assert_eq!(result.first_byte_ms.is_some(), backend == Backend::Remote);
        if backend == Backend::Local {
            assert!(result.response.model.starts_with("local-laya/"));
        }
    }
}
