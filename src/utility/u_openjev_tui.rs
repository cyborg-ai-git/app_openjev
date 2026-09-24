use crate::app::{App, EnumOpenjevAction as Action, Kind};
use crate::{Answer, EnumOpenjevBackend as Backend};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
};

const BG: Color = Color::Rgb(12, 17, 27);
const SURFACE: Color = Color::Rgb(18, 26, 39);
const FG: Color = Color::Rgb(226, 234, 245);
const MUTED: Color = Color::Rgb(139, 158, 184);
const ACCENT: Color = Color::Rgb(87, 219, 181);
const LINE: Color = Color::Rgb(48, 65, 87);

fn panel(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .title_style(
            Style::default()
                .fg(if focused { ACCENT } else { MUTED })
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(SURFACE).fg(FG))
        .border_style(Style::default().fg(if focused { ACCENT } else { LINE }))
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.mouse_regions.clear();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    if area.width < 70 || area.height < 24 {
        frame.render_widget(
            Paragraph::new("OPENJEV\nResize to at least 70 x 24.\nCtrl+Q exits.")
                .block(panel(" Window too small ", false)),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(13),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(area);
    frame.render_widget(panel(" OPENJEV / Decision studio ", false), rows[0]);
    let header = rows[0].inner(Margin::new(1, 1));
    let next_x = buttons(
        frame,
        app,
        header,
        &[
            (
                "Remote",
                Action::Backend(Backend::Remote),
                app.backend == Backend::Remote,
            ),
            (
                "Local",
                Action::Backend(Backend::Local),
                app.backend == Backend::Local,
            ),
            (
                "Demo",
                Action::Backend(Backend::Demo),
                app.backend == Backend::Demo,
            ),
        ],
    );
    let model = Rect::new(next_x, header.y, header.right().saturating_sub(next_x), 1);
    frame.render_widget(
        Paragraph::new(format!(" {}  |  edit model...", app.backend_status()))
            .style(Style::default().fg(ACCENT)),
        model,
    );
    app.mouse_regions.push((model, Action::Settings));
    frame.render_widget(panel(" QUESTION TYPE ", false), rows[1]);
    buttons(
        frame,
        app,
        rows[1].inner(Margin::new(1, 1)),
        &[
            (Kind::Choice.label(), Action::Kind(0), app.selected == 0),
            (Kind::Score.label(), Action::Kind(1), app.selected == 1),
            (Kind::Noul.label(), Action::Kind(2), app.selected == 2),
            (
                if app.json_state {
                    "JSON state ^B"
                } else {
                    "Text state ^B"
                },
                Action::StateFormat,
                app.json_state,
            ),
        ],
    );
    if let Some(index) = app.expanded {
        if index == 3 {
            render_result(frame, app, rows[2]);
        } else {
            render_editor(
                frame,
                app,
                rows[2],
                index,
                format!(" {} / expanded · Esc restores layout ", field_name(index)),
            );
        }
    } else {
        let (input, output) = if area.width >= 105 {
            let columns = Layout::horizontal([
                Constraint::Percentage(53),
                Constraint::Length(1),
                Constraint::Min(35),
            ])
            .split(rows[2]);
            (columns[0], columns[2])
        } else {
            let columns = Layout::vertical([
                Constraint::Length((rows[2].height * 3 / 5).max(9)),
                Constraint::Min(4),
            ])
            .split(rows[2]);
            (columns[0], columns[1])
        };
        let fields = if input.height < 13 {
            Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(3),
                Constraint::Min(3),
            ])
        } else {
            Layout::vertical([
                Constraint::Percentage(43),
                Constraint::Percentage(23),
                Constraint::Percentage(34),
            ])
        }
        .split(input);
        let titles = [
            format!(
                " STATE / {} ",
                if app.json_state { "JSON" } else { "plain text" }
            ),
            " QUESTION / instructions ".into(),
            match app.kind() {
                Kind::Choice => " CRITERIA / key = description ",
                Kind::Score => " CRITERIA / one level per line ",
                Kind::Noul => " CRITERIA / true or false = description ",
            }
            .into(),
        ];
        for index in 0..3 {
            render_editor(frame, app, fields[index], index, titles[index].clone());
        }
        render_result(frame, app, output);
    }
    let status = if let Some(start) = app.started {
        format!(
            "WORKING  {:.1}s  ·  {}",
            start.elapsed().as_secs_f32(),
            app.status
        )
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(if app.error { Color::LightRed } else { MUTED }))
            .wrap(Wrap { trim: false })
            .block(panel(
                if app.error { " ATTENTION " } else { " STATUS " },
                false,
            )),
        rows[3],
    );
    buttons(
        frame,
        app,
        Rect::new(rows[4].x, rows[4].y, rows[4].width, 1),
        &[
            ("Run ^G", Action::Evaluate, true),
            ("Cancel", Action::Cancel, false),
            ("JSON", Action::Preview, app.raw),
            ("Export", Action::Export, false),
            ("Settings", Action::Settings, false),
            ("Help", Action::Help, false),
            ("Quit", Action::Quit, false),
        ],
    );
    edit_toolbar(
        frame,
        app,
        Rect::new(rows[4].x, rows[4].y + 1, rows[4].width, 1),
    );
    if app.settings.is_some() {
        settings(frame, app, area);
    }
    if app.help {
        app.mouse_regions.clear();
        let rect = help(frame, area);
        app.mouse_regions
            .push((Rect::new(rect.x, rect.y, rect.width, 1), Action::Help));
    }
}

fn field_name(index: usize) -> &'static str {
    match index {
        0 => "STATE",
        1 => "QUESTION",
        2 => "CRITERIA",
        4 => "REMOTE MODEL",
        5 => "LOCAL CHECKPOINT",
        _ => "EXPORT FOLDER",
    }
}

fn render_editor(frame: &mut Frame, app: &mut App, rect: Rect, index: usize, title: String) {
    let previous = app.focus;
    let focused = previous == index;
    let hovered = app.pointer.is_some_and(|point| rect.contains(point));
    app.focus = index;
    let editor = app.active_editor().expect("Visible editor exists");
    let cursor = editor.cursor();
    let mut block = panel(title, focused).padding(Padding::horizontal(1));
    if focused && rect.width >= 45 {
        block = block.title_bottom(
            Line::from(format!(
                " Ln {}  Col {} · editable ",
                cursor.0 + 1,
                cursor.1 + 1
            ))
            .style(Style::default().fg(ACCENT)),
        );
    } else if hovered {
        block = block.border_style(Style::default().fg(MUTED));
    }
    let inner = block.inner(rect);
    editor.set_block(block);
    editor.set_style(Style::default().fg(FG).bg(SURFACE));
    editor.set_cursor_line_style(Style::default());
    editor.set_cursor_style(if focused {
        Style::default().bg(ACCENT).fg(BG)
    } else {
        Style::default()
    });
    editor.set_selection_style(
        Style::default()
            .bg(Color::Rgb(43, 79, 114))
            .fg(Color::White),
    );
    editor.set_placeholder_text("Click here to enter text...");
    frame.render_widget(&*editor, rect);
    app.focus = previous;
    app.mouse_regions.push((rect, Action::Focus(index)));
    app.mouse_regions.push((inner, Action::Editor(index)));
}

fn render_result(frame: &mut Frame, app: &mut App, rect: Rect) {
    let title = app.last.as_ref().map_or_else(
        || " RESULT / read-only ".into(),
        |result| format!(" {} result / read-only ", result.backend.label()),
    );
    let block = panel(title, app.focus == 3).padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    app.mouse_regions.push((rect, Action::Results));
    let metric_height = if inner.height >= 8 { 3 } else { 1 }.min(inner.height);
    let metrics = Rect::new(inner.x, inner.y, inner.width, metric_height);
    let text = if let Some(result) = &app.last {
        let first = result
            .first_byte_ms
            .map_or_else(|| "N/A".into(), |ms| format!("{ms} ms"));
        if metric_height == 1 {
            format!(
                "Response: {} ms · TTFB: {first} · TTFT: N/A",
                result.elapsed_ms
            )
        } else {
            format!(
                "Response: {} ms\nTTFB: {first}\nTTFT: N/A (no token stream)",
                result.elapsed_ms
            )
        }
    } else {
        "Response: -- · TTFT: N/A".into()
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(ACCENT)),
        metrics,
    );
    let content = Rect::new(
        inner.x,
        inner.y + metric_height,
        inner.width,
        inner.height.saturating_sub(metric_height),
    );
    let lines = if app.raw {
        app.last
            .as_ref()
            .and_then(|result| serde_json::to_string_pretty(&result.response).ok())
            .unwrap_or_else(|| "No result yet.".into())
            .lines()
            .map(|line| Line::raw(line.to_owned()))
            .collect()
    } else {
        result_lines(app)
    };
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(FG).bg(SURFACE))
        .wrap(Wrap { trim: false });
    app.scroll = app.scroll.min(
        paragraph
            .line_count(content.width)
            .saturating_sub(content.height as usize)
            .min(u16::MAX as usize) as u16,
    );
    frame.render_widget(paragraph.scroll((app.scroll, 0)), content);
}

fn edit_toolbar(frame: &mut Frame, app: &mut App, row: Rect) {
    buttons(
        frame,
        app,
        row,
        &[
            ("All", Action::SelectAll, false),
            ("Copy", Action::Copy, false),
            ("Cut", Action::Cut, false),
            ("Paste", Action::Paste, false),
            ("Undo", Action::Undo, false),
            ("Redo", Action::Redo, false),
            ("Clear", Action::Clear, false),
            (
                if app.expanded.is_some() {
                    "Restore"
                } else {
                    "Expand"
                },
                Action::Expand,
                app.expanded.is_some(),
            ),
        ],
    );
}

fn settings(frame: &mut Frame, app: &mut App, area: Rect) {
    app.mouse_regions.clear();
    frame.render_widget(
        Block::default().style(Style::default().fg(LINE).bg(BG)),
        area,
    );
    let width = area.width.saturating_sub(4).min(86);
    let height = area.height.saturating_sub(2).min(22);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let block = panel(" SESSION SETTINGS / Ctrl+S applies · Esc cancels ", true);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(" Credentials stay in your private Evo file.")
            .style(Style::default().fg(MUTED)),
        rows[0],
    );
    render_editor(frame, app, rows[1], 4, " Remote model ".into());
    render_editor(frame, app, rows[2], 5, " Local checkpoint folder ".into());
    let device = app.settings.as_ref().expect("Settings open").device.clone();
    let mut devices = vec![
        ("Auto", Action::Device("auto"), device == "auto"),
        ("CPU", Action::Device("cpu"), device == "cpu"),
    ];
    if cfg!(feature = "metal") {
        devices.push(("Metal", Action::Device("metal"), device == "metal"));
    }
    if cfg!(feature = "cuda") {
        devices.push(("CUDA", Action::Device("cuda"), device == "cuda"));
    }
    buttons(frame, app, rows[3], &devices);
    render_editor(frame, app, rows[4], 6, " Export folder ".into());
    let message = if app.error {
        app.status.as_str()
    } else {
        "Changes apply to this session. A new local path or device reloads Laya in the background."
    };
    frame.render_widget(
        Paragraph::new(message)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(if app.error { Color::LightRed } else { MUTED })),
        rows[5],
    );
    buttons(
        frame,
        app,
        rows[6],
        &[
            ("Apply ^S", Action::ApplySettings, true),
            ("Cancel Esc", Action::CloseSettings, false),
        ],
    );
    // Editing actions operate on the focused settings field as well.
    buttons(
        frame,
        app,
        rows[7],
        &[
            ("All", Action::SelectAll, false),
            ("Copy", Action::Copy, false),
            ("Cut", Action::Cut, false),
            ("Paste", Action::Paste, false),
            ("Undo", Action::Undo, false),
            ("Redo", Action::Redo, false),
        ],
    );
}

pub fn result_lines(app: &App) -> Vec<Line<'static>> {
    let Some(result) = &app.last else {
        return vec![
            Line::raw("  One question. One structured decision."),
            Line::raw(""),
            Line::raw("  1. Type or paste the state."),
            Line::raw("  2. Set the question and criteria."),
            Line::raw("  3. Press Ctrl+G."),
            Line::raw(""),
            Line::raw("  Choice: select a category"),
            Line::raw("  Score: evaluate an ordered scale"),
            Line::raw("  Noul: probability of true"),
            Line::raw(""),
            Line::raw("  Results stay linked to the submitted request."),
        ];
    };
    let mut lines = Vec::new();
    lines.extend(vec![
        Line::styled(result.response.model.clone(), Style::default().fg(ACCENT)),
        Line::raw(format!(
            "{} ms · input {} / output {} token",
            result.elapsed_ms,
            result.response.usage.input_tokens,
            result.response.usage.output_tokens
        )),
        Line::raw(""),
    ]);
    if result.demo {
        lines.push(Line::styled(
            "SIMULATED DATA: does not interpret the input.",
            Style::default().fg(Color::Yellow),
        ));
        lines.push(Line::raw(""));
    }
    for (id, answer) in &result.response.answers {
        lines.push(Line::styled(
            id.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        match answer {
            Answer::Noul { noul } => {
                lines.push(Line::raw(format!("P(true): {:.1}%", noul * 100.0)));
                lines.push(bar("true", *noul));
                lines.push(bar("false", 1.0 - noul));
            }
            Answer::Choice {
                choice,
                probabilities,
                confidence,
            } => {
                lines.push(Line::raw(format!("Choice: {choice}")));
                lines.push(Line::raw(format!("Confidence: {confidence:.3}")));
                lines.push(Line::raw(""));
                lines.extend(probabilities.iter().map(|(k, p)| bar(k, *p)));
            }
            Answer::Score {
                score,
                legend,
                probabilities,
                confidence,
            } => {
                lines.push(Line::raw(format!(
                    "Score: {score:.3} · confidence: {confidence:.3}"
                )));
                lines.push(Line::raw(""));
                for (key, p) in probabilities {
                    let label = format!("{key}: {}", legend.get(key).map_or("", String::as_str));
                    lines.push(bar(&label, *p));
                }
            }
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "Confidence ≠ probability of correctness.",
        Style::default().fg(MUTED),
    ));
    lines.push(Line::styled(
        "Ctrl+O also exports the original request.",
        Style::default().fg(MUTED),
    ));
    lines
}

fn bar(label: &str, p: f64) -> Line<'static> {
    let filled = (p.clamp(0.0, 1.0) * 12.0).round() as usize;
    Line::from(vec![
        Span::styled(
            format!("{}{}", "█".repeat(filled), "░".repeat(12 - filled)),
            Style::default().fg(ACCENT),
        ),
        Span::raw(format!(" {:5.1}%  {label}", p * 100.0)),
    ])
}

fn help(frame: &mut Frame, area: Rect) -> Rect {
    frame.render_widget(
        Block::default().style(Style::default().fg(LINE).bg(BG)),
        area,
    );
    let width = area.width.min(84).saturating_sub(4);
    let height = area.height.min(24).saturating_sub(2);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let text = "Click a field to edit; drag to select; double-click a word.\nShift+click extends selection; wheel scrolls under pointer.\nCtrl+A all · Ctrl+C copy · Ctrl+X cut · Ctrl+V paste\nCtrl+Z undo · Ctrl+Y redo · Cmd+V terminal paste\nCopy/Cut/Paste use the macOS system clipboard.\nCtrl+E expands the active panel; Esc restores the layout.\nCtrl+K opens model/path settings; Ctrl+S applies them.\nTab / Shift+Tab switch editable fields and results.\nCtrl+D switches Remote/Local; Ctrl+T changes question type.\nCtrl+B text/JSON state · Ctrl+G evaluate · Esc cancel wait\nCtrl+P result JSON · Ctrl+O export · Ctrl+Q quit\nClick results then Copy to copy response JSON.\n\nAll input fields are editable; results retain their snapshot.\nRemote calls the API; Local uses Laya; Demo is simulated.\nTTFT is N/A: no token stream. TTFB is the first body byte.\nThe first local load runs in the background; weights stay loaded.";
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(SURFACE).fg(FG))
            .wrap(Wrap { trim: false })
            .block(
                panel(" Help / click this title or Esc to close ", true)
                    .padding(Padding::horizontal(1)),
            ),
        rect,
    );
    rect
}

fn buttons(frame: &mut Frame, app: &mut App, row: Rect, entries: &[(&str, Action, bool)]) -> u16 {
    let mut x = row.x;
    for (label, action, selected) in entries {
        let text = format!(" {label} ");
        let width = text.len() as u16;
        if x.saturating_add(width) > row.right() {
            break;
        }
        let rect = Rect::new(x, row.y, width, row.height.min(1));
        let hovered = app.pointer.is_some_and(|point| rect.contains(point));
        let style = if *selected {
            Style::default()
                .bg(ACCENT)
                .fg(BG)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default()
                .bg(Color::Rgb(53, 79, 101))
                .fg(Color::White)
        } else {
            Style::default().fg(FG).bg(Color::Rgb(32, 44, 61))
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        app.mouse_regions.push((rect, *action));
        x += width + 1;
    }
    x.min(row.right())
}
