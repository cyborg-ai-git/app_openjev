use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use openjev::{
    app::{App, Completed, demo_response},
    ui,
};
use ratatui::{Terminal, backend::TestBackend};

fn app() -> App {
    App::new(
        "jev-latest".into(),
        "Unicode input: urgent café request 🦀",
        true,
        None,
        ".".into(),
    )
}
fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn renders_all_modes_at_multiple_sizes_without_panics() {
    for (w, h) in [(120, 36), (80, 32), (70, 24), (30, 8), (1, 1)] {
        for mode in 0..3 {
            let mut app = app();
            app.selected = mode;
            let request = app.request().unwrap();
            app.last = Some(Completed {
                response: demo_response(&request),
                request,
                demo: true,
                elapsed_ms: 4,
            });
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
            app.raw = true;
            app.scroll = u16::MAX;
            terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
            app.help = true;
            terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
    }
}

#[test]
fn compact_terminal_keeps_every_input_visible() {
    let mut app = app();
    let mut terminal = Terminal::new(TestBackend::new(70, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let visible: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(visible.contains("Unicode input"));
    assert!(visible.contains("Which team"));
    assert!(visible.contains("billing = Payments"));
}

#[test]
fn mode_switch_preserves_edits_and_help_blocks_input() {
    let mut app = app();
    app.focus = 1;
    app.key(key(KeyCode::Char('X')));
    let original = app.forms[0].instruction.lines().to_vec();
    for _ in 0..3 {
        app.key(key(KeyCode::F(2)));
    }
    assert_eq!(app.forms[0].instruction.lines(), original);
    app.key(key(KeyCode::F(1)));
    app.key(key(KeyCode::Char('Y')));
    assert_eq!(app.forms[0].instruction.lines(), original);
    app.key(key(KeyCode::Esc));
    assert!(!app.help);
}

#[tokio::test]
async fn demo_can_be_cancelled_and_export_keeps_original_request() {
    let mut app = app();
    app.submit();
    app.cancel();
    assert!(app.pending.is_none());
    app.submit();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    app.collect().await;
    assert!(app.last.is_some());
    app.state = openjev::app::editor("Subsequent edit");
    let temp = tempfile::tempdir().unwrap();
    app.export_dir = temp.path().to_path_buf();
    let path = app.export().unwrap();
    let export: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        export["request"]["state"],
        "Unicode input: urgent café request 🦀"
    );
    assert_eq!(export["demo"], true);
}
