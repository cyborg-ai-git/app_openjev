//! Editor hit testing uses the textarea's Unicode and wrapping model.
use ratatui::layout::Rect;
use ratatui_textarea::{CursorMove, TextArea};

pub fn select_all(editor: &mut TextArea<'_>) {
    // TextArea::select_all uses a u16 jump and truncates very long logical lines.
    editor.cancel_selection();
    editor.move_cursor(CursorMove::Jump(0, 0));
    editor.start_selection();
    editor.move_cursor(CursorMove::Bottom);
    editor.move_cursor(CursorMove::End);
}

pub fn place_cursor(editor: &mut TextArea<'_>, rect: Rect, column: u16, row: u16) {
    if rect.is_empty() {
        return;
    }
    editor.move_cursor(CursorMove::Jump(0, 0));
    editor.move_cursor(CursorMove::InViewport);
    let left = editor.screen_cursor().col;
    for _ in 0..row.saturating_sub(rect.y).min(rect.height - 1) {
        editor.move_cursor(CursorMove::Down);
    }
    let target_row = editor.screen_cursor().row;
    let target_col = left + column.saturating_sub(rect.x).min(rect.width - 1) as usize;
    while editor.screen_cursor().col < target_col {
        let previous = editor.cursor();
        editor.move_cursor(CursorMove::Forward);
        let screen = editor.screen_cursor();
        if screen.row != target_row || screen.col > target_col {
            editor.move_cursor(CursorMove::Back);
            break;
        }
        if editor.cursor() == previous {
            break;
        }
    }
}

pub fn select_word(editor: &mut TextArea<'_>) {
    editor.cancel_selection();
    let cursor = editor.cursor();
    let chars: Vec<char> = editor.lines()[cursor.0].chars().collect();
    if chars.is_empty() {
        return;
    }
    let index = cursor.1.min(chars.len() - 1);
    let category = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let kind = category(chars[index]);
    let mut start = index;
    let mut end = index + 1;
    while start > 0 && category(chars[start - 1]) == kind {
        start -= 1;
    }
    while end < chars.len() && category(chars[end]) == kind {
        end += 1;
    }
    for _ in start..cursor.1 {
        editor.move_cursor(CursorMove::Back);
    }
    editor.start_selection();
    for _ in start..end {
        editor.move_cursor(CursorMove::Forward);
    }
}

/// Native clipboard access never passes text as command-line arguments.
pub fn copy_system(text: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let mut child = Command::new("/usr/bin/pbcopy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| anyhow::anyhow!("Could not open the macOS clipboard"))?;
        let write = child
            .stdin
            .take()
            .expect("Piped clipboard input")
            .write_all(text.as_bytes());
        let status = child.wait();
        anyhow::ensure!(
            write.is_ok() && status.is_ok_and(|status| status.success()),
            "Could not copy text to the macOS clipboard"
        );
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        anyhow::bail!(
            "Native clipboard unavailable; use the application clipboard or terminal paste"
        )
    }
}

pub fn paste_system() -> anyhow::Result<String> {
    #[cfg(target_os = "macos")]
    {
        use std::{
            io::Read,
            process::{Command, Stdio},
        };
        let mut child = Command::new("/usr/bin/pbpaste")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| anyhow::anyhow!("Could not open the macOS clipboard"))?;
        let mut text = String::new();
        const LIMIT: u64 = 16 * 1024 * 1024;
        let read = child
            .stdout
            .take()
            .expect("Piped clipboard output")
            .take(LIMIT + 1)
            .read_to_string(&mut text);
        if read.is_err() || text.len() as u64 > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Clipboard must contain UTF-8 text under 16 MiB");
        }
        anyhow::ensure!(
            child.wait().is_ok_and(|status| status.success()),
            "Could not read the macOS clipboard"
        );
        Ok(text)
    }
    #[cfg(not(target_os = "macos"))]
    {
        anyhow::bail!("Native clipboard unavailable; use terminal paste")
    }
}
