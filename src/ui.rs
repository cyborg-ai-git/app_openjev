use crate::Answer;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
};

use crate::app::{App, Kind};

const BG: Color = Color::Rgb(13, 19, 31);
const FG: Color = Color::Rgb(218, 229, 241);
const MUTED: Color = Color::Rgb(134, 153, 175);
const ACCENT: Color = Color::Rgb(81, 220, 186);

fn panel(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(if focused {
            ACCENT
        } else {
            Color::Rgb(51, 68, 91)
        }))
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    if area.width < 70 || area.height < 24 {
        frame.render_widget(
            Paragraph::new("OPENJEV\nResize the terminal to at least 70 x 24.\nCtrl+Q exits.")
                .block(panel(" Small terminal ", false)),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(area);
    let mode = if let Some(label) = &app.local_label {
        label.as_str()
    } else if app.demo {
        "DEMO · offline simulation"
    } else if app.client.is_some() {
        "API · key configured"
    } else {
        "API · missing key"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " OPENJEV ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {mode}  |  {}", app.model)),
        ]))
        .block(panel(" TypeSafe AI / decision workbench ", false)),
        rows[0],
    );
    frame.render_widget(
        Tabs::new(Kind::ALL.map(Kind::label))
            .select(app.selected)
            .highlight_style(
                Style::default()
                    .bg(ACCENT)
                    .fg(BG)
                    .add_modifier(Modifier::BOLD),
            )
            .divider("  ")
            .block(panel(
                " F2 changes type · each type keeps its editor ",
                false,
            )),
        rows[1],
    );
    let columns = if area.width >= 105 {
        Layout::horizontal([Constraint::Percentage(53), Constraint::Percentage(47)]).split(rows[2])
    } else {
        Layout::vertical([
            Constraint::Length((rows[2].height * 3 / 5).max(9)),
            Constraint::Min(4),
        ])
        .split(rows[2])
    };
    let fields = if columns[0].height < 13 {
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
    .split(columns[0]);
    let state_title = format!(
        " 1 / State · {} · F4 changes format ",
        if app.json_state { "JSON" } else { "text" }
    );
    let criteria_title = match app.kind() {
        Kind::Choice => " 3 / Options: key = description ",
        Kind::Score => " 3 / Ordered levels: one per line (2-10) ",
        Kind::Noul => " 3 / Optional criteria: true/false = description ",
    };
    let titles = [state_title, " 2 / Question ".into(), criteria_title.into()];
    let focus = app.focus;
    let selected = app.selected;
    let form = &mut app.forms[selected];
    for (i, editor) in [&mut app.state, &mut form.instruction, &mut form.criteria]
        .into_iter()
        .enumerate()
    {
        editor.set_block(panel(titles[i].clone(), focus == i));
        editor.set_style(Style::default().fg(FG).bg(BG));
        editor.set_cursor_line_style(Style::default());
        editor.set_cursor_style(if focus == i {
            Style::default().bg(ACCENT).fg(BG)
        } else {
            Style::default()
        });
        frame.render_widget(&*editor, fields[i]);
    }
    let mut lines = result_lines(app);
    if app.raw {
        lines = app
            .last
            .as_ref()
            .and_then(|r| serde_json::to_string_pretty(&r.response).ok())
            .unwrap_or_else(|| "No response to display.".into())
            .lines()
            .map(|s| Line::raw(s.to_owned()))
            .collect();
    }
    let title = if app.last.as_ref().is_some_and(|r| r.demo) {
        " 4 / Latest evaluation · SIMULATED DEMO "
    } else {
        " 4 / Latest evaluation · F6 JSON "
    };
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(panel(title, focus == 3));
    let inner_width = columns[1].width.saturating_sub(2);
    let visible_lines = columns[1].height.saturating_sub(2) as usize;
    let max_scroll = paragraph
        .line_count(inner_width)
        .saturating_sub(visible_lines)
        .min(u16::MAX as usize) as u16;
    app.scroll = app.scroll.min(max_scroll);
    frame.render_widget(paragraph.scroll((app.scroll, 0)), columns[1]);
    let status = if let Some(start) = app.started {
        format!("{}  {:.1}s", app.status, start.elapsed().as_secs_f32())
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(if app.error { Color::LightRed } else { ACCENT }))
            .wrap(Wrap { trim: false })
            .block(panel(" Status ", false)),
        rows[3],
    );
    frame.render_widget(Paragraph::new(" Tab field  F5 evaluate  Esc cancel  F6 JSON  F7 export\n F1 help    PgUp/PgDn results    Ctrl+Q quit").style(Style::default().fg(MUTED)), rows[4]);
    if app.help {
        help(frame, area);
    }
}

pub fn result_lines(app: &App) -> Vec<Line<'static>> {
    let Some(result) = &app.last else {
        return vec![
            Line::raw(""),
            Line::raw("  One question. One structured decision."),
            Line::raw(""),
            Line::raw("  1. Type or paste the state."),
            Line::raw("  2. Set the question and criteria."),
            Line::raw("  3. Press F5."),
            Line::raw(""),
            Line::raw("  Choice: select a category"),
            Line::raw("  Score: evaluate an ordered scale"),
            Line::raw("  Noul: probability of true"),
            Line::raw(""),
            Line::raw("  Results stay linked to the submitted request."),
        ];
    };
    let mut lines = vec![
        Line::styled(result.response.model.clone(), Style::default().fg(ACCENT)),
        Line::raw(format!(
            "{} ms · input {} / output {} token",
            result.elapsed_ms,
            result.response.usage.input_tokens,
            result.response.usage.output_tokens
        )),
        Line::raw(""),
    ];
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
        "F7 also exports the original request.",
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

fn help(frame: &mut Frame, area: Rect) {
    let width = area.width.min(80).saturating_sub(4);
    let height = area.height.min(23).saturating_sub(2);
    let rect = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let text = "Tab / Shift+Tab    Switch fields\nF2                 Choice → Score → Noul\nF4                 Plain text / JSON state\nF5 / Ctrl+Enter     Evaluate\nEsc                Cancel wait / close help\nF6                 Toggle results / JSON\nF7                 Export request and response to a new file\nPgUp / PgDn        Scroll results\nCtrl+Q / Ctrl+C    Quit\n\nEditor: arrows, Home/End, Enter, Shift to select.\nCtrl+U undoes an edit; Ctrl+R redoes it.\nUse terminal paste (Cmd+V on macOS).\n\nDemo uses fixed values, not a local AI model.\nAPI mode sends the state to TypeSafe.\nLocal mode uses Laya, an independent open model.";
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(BG).fg(FG))
            .wrap(Wrap { trim: false })
            .block(panel(" Help · F1 / Esc closes ", true)),
        rect,
    );
}
