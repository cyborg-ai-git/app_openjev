use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use openjev::{
    app::{App, EnumOpenjevAction as Action, editor},
    ui,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

fn app() -> App {
    App::new(
        "jev-1.13.0".into(),
        "alpha βeta 🦀 delta",
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
fn rect(app: &App, action: Action) -> Rect {
    app.mouse_regions
        .iter()
        .find(|(_, target)| *target == action)
        .unwrap()
        .0
}
fn mouse(app: &mut App, area: Rect, x: u16, y: u16, kind: MouseEventKind, modifiers: KeyModifiers) {
    app.mouse(MouseEvent {
        kind,
        column: area.x + x,
        row: area.y + y,
        modifiers,
    });
}
fn click(app: &mut App, action: Action) {
    let area = rect(app, action);
    mouse(
        app,
        area,
        0,
        0,
        MouseEventKind::Down(MouseButton::Left),
        KeyModifiers::NONE,
    );
    mouse(
        app,
        area,
        0,
        0,
        MouseEventKind::Up(MouseButton::Left),
        KeyModifiers::NONE,
    );
}
fn control(app: &mut App, character: char) -> bool {
    app.key(KeyEvent::new(
        KeyCode::Char(character),
        KeyModifiers::CONTROL,
    ))
}

#[test]
fn every_input_supports_typing_deletion_multiline_paste_and_toolbar_editing() {
    let mut app = app();
    for kind in 0..3 {
        app.selected = kind;
        for field in 0..3 {
            draw(&mut app, 120, 36);
            click(&mut app, Action::Focus(field));
            assert_eq!(app.focus, field);
            click(&mut app, Action::Clear);
            assert!(app.active_editor().unwrap().is_empty());
            app.paste_text("first café\r\nsecond 🦀");
            assert_eq!(
                app.active_editor().unwrap().lines(),
                ["first café", "second 🦀"]
            );
            app.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
            assert_eq!(app.active_editor().unwrap().lines()[1], "second ");
            click(&mut app, Action::Undo);
            assert_eq!(app.active_editor().unwrap().lines()[1], "second 🦀");
            click(&mut app, Action::Redo);
            assert_eq!(app.active_editor().unwrap().lines()[1], "second ");
            click(&mut app, Action::SelectAll);
            click(&mut app, Action::Copy);
            assert_eq!(app.clipboard, "first café\nsecond ");
            assert!(app.active_editor().unwrap().selection_range().is_some());
            click(&mut app, Action::Cut);
            assert!(app.active_editor().unwrap().is_empty());
            click(&mut app, Action::Paste);
            assert_eq!(
                app.active_editor().unwrap().lines(),
                ["first café", "second "]
            );
        }
    }
}

#[test]
fn drag_shift_click_and_double_click_select_text_without_losing_unicode() {
    let mut app = app();
    draw(&mut app, 120, 36);
    let area = rect(&app, Action::Editor(0));
    mouse(
        &mut app,
        area,
        0,
        0,
        MouseEventKind::Down(MouseButton::Left),
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        area,
        5,
        0,
        MouseEventKind::Drag(MouseButton::Left),
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        area,
        5,
        0,
        MouseEventKind::Up(MouseButton::Left),
        KeyModifiers::NONE,
    );
    assert_eq!(app.state.selection_range(), Some(((0, 0), (0, 5))));
    assert!(!control(&mut app, 'c'));
    assert_eq!(app.clipboard, "alpha");
    assert!(app.pending.is_none());
    mouse(
        &mut app,
        area,
        2,
        0,
        MouseEventKind::Down(MouseButton::Left),
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        area,
        8,
        0,
        MouseEventKind::Down(MouseButton::Left),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.state.selection_range(), Some(((0, 2), (0, 8))));
    for _ in 0..2 {
        mouse(
            &mut app,
            area,
            7,
            0,
            MouseEventKind::Down(MouseButton::Left),
            KeyModifiers::NONE,
        );
    }
    control(&mut app, 'c');
    assert_eq!(app.clipboard, "βeta");
    app.focus = 1;
    control(&mut app, 'a');
    control(&mut app, 'v');
    assert_eq!(app.forms[0].instruction.lines(), ["βeta"]);
}

#[test]
fn selection_and_clear_cover_long_inputs_and_clear_is_undoable() {
    let mut app = app();
    let text = "a".repeat(90_000);
    app.state = editor(&text);
    control(&mut app, 'a');
    control(&mut app, 'c');
    assert_eq!(app.clipboard.len(), text.len());
    app.edit_action(Action::Clear).unwrap();
    assert!(app.state.is_empty());
    control(&mut app, 'z');
    assert_eq!(app.state.lines()[0], text);
}

#[test]
fn expanded_editor_preserves_text_and_restores_layout_on_escape() {
    let mut app = app();
    app.focus = 2;
    control(&mut app, 'e');
    assert_eq!(app.expanded, Some(2));
    draw(&mut app, 70, 24);
    assert!(rect(&app, Action::Editor(2)).height >= 10);
    assert!(
        !app.mouse_regions
            .iter()
            .any(|(_, action)| *action == Action::Editor(0))
    );
    let original = app.forms[0].criteria.lines().to_vec();
    app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.expanded, Some(3));
    app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.expanded, Some(0));
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    draw(&mut app, 70, 24);
    assert!(app.expanded.is_none());
    assert_eq!(app.forms[0].criteria.lines(), original);
    assert!(
        app.mouse_regions
            .iter()
            .any(|(_, action)| *action == Action::Editor(0))
    );
}

#[tokio::test]
async fn settings_are_editable_modal_validated_and_applied_atomically() {
    let mut app = app();
    let directory = tempfile::tempdir().unwrap();
    draw(&mut app, 120, 36);
    click(&mut app, Action::Settings);
    draw(&mut app, 70, 24);
    assert_eq!(app.focus, 4);
    assert!(
        !app.mouse_regions
            .iter()
            .any(|(_, action)| *action == Action::Evaluate)
    );
    for (index, value) in [
        (4, "jev-preview"),
        (5, "models/laya"),
        (6, directory.path().to_str().unwrap()),
    ] {
        click(&mut app, Action::Editor(index));
        control(&mut app, 'a');
        app.paste_text(value);
        assert_eq!(app.active_editor().unwrap().lines(), [value]);
    }
    click(&mut app, Action::Device("cpu"));
    click(&mut app, Action::ApplySettings);
    assert!(app.settings.is_none());
    assert_eq!(app.model, "jev-preview");
    assert_eq!(app.export_dir, directory.path());
    assert_eq!(app.request().unwrap().model, "jev-preview");
    control(&mut app, 'k');
    control(&mut app, 'a');
    app.paste_text("invalid model");
    let old_directory = app.export_dir.clone();
    assert!(app.apply_settings().is_err());
    assert!(app.settings.is_some());
    assert_eq!(app.model, "jev-preview");
    assert_eq!(app.export_dir, old_directory);
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.settings.is_none());
    assert_eq!(app.model, "jev-preview");
}

#[test]
fn timing_stays_visible_while_results_scroll_and_all_controls_fit() {
    let mut app = app();
    let request = app.request().unwrap();
    app.last = Some(openjev::app::Completed {
        backend: openjev::EnumOpenjevBackend::Demo,
        ttft_ms: None,
        first_byte_ms: None,
        demo: true,
        elapsed_ms: 350,
        response: openjev::app::demo_response(&request),
        request,
    });
    for (width, height) in [(70, 24), (80, 32), (120, 36), (160, 48)] {
        app.scroll = u16::MAX;
        let screen = draw(&mut app, width, height);
        assert!(screen.contains("Response: 350 ms") && screen.contains("TTFT: N/A"));
        for action in [
            Action::SelectAll,
            Action::Copy,
            Action::Cut,
            Action::Paste,
            Action::Undo,
            Action::Redo,
            Action::Clear,
            Action::Expand,
            Action::Evaluate,
            Action::Settings,
            Action::Quit,
        ] {
            let area = rect(&app, action);
            assert!(area.right() <= width && area.bottom() <= height);
        }
    }
}
