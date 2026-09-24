use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{Answer, Client, EnumOpenjevBackend as Backend, Evaluation, Question, Request, Usage};
use anyhow::{Context, Result, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui_textarea::{TextArea, WrapMode};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Choice,
    Score,
    Noul,
}

impl Kind {
    pub const ALL: [Self; 3] = [Self::Choice, Self::Score, Self::Noul];
    pub fn label(self) -> &'static str {
        match self {
            Self::Choice => "Choice",
            Self::Score => "Score",
            Self::Noul => "Noul",
        }
    }
}

pub struct Form {
    pub instruction: TextArea<'static>,
    pub criteria: TextArea<'static>,
}

pub fn editor(text: &str) -> TextArea<'static> {
    let mut area = TextArea::from(text.lines());
    area.set_wrap_mode(WrapMode::Word);
    area
}

fn forms() -> [Form; 3] {
    [
        Form {
            instruction: editor("Which team should handle this message?"),
            criteria: editor(
                "billing = Payments, invoices and refunds\ntechnical = Bugs and technical problems\nother = Other requests",
            ),
        },
        Form {
            instruction: editor("How frustrated does the customer appear?"),
            criteria: editor("Calm\nFrustrated but polite\nVery angry"),
        },
        Form {
            instruction: editor("Does the message express an urgent request?"),
            criteria: editor("true = Explicitly urgent request\nfalse = No urgency expressed"),
        },
    ]
}

#[derive(Serialize)]
pub struct Completed {
    pub backend: Backend,
    /// None: neither the structured API nor the local classifier streams tokens.
    pub ttft_ms: Option<u128>,
    pub first_byte_ms: Option<u128>,
    pub demo: bool,
    pub elapsed_ms: u128,
    pub request: Request,
    pub response: Evaluation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumOpenjevAction {
    Backend(Backend),
    Kind(usize),
    Editor(usize),
    Results,
    Evaluate,
    Cancel,
    Preview,
    Export,
    Help,
    StateFormat,
    Switch,
    Quit,
    Focus(usize),
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    SelectAll,
    Clear,
    Expand,
    Settings,
    ApplySettings,
    CloseSettings,
    Device(&'static str),
}

pub struct UOpenjevSettingsDraft {
    pub model: TextArea<'static>,
    pub local_path: TextArea<'static>,
    pub export_path: TextArea<'static>,
    pub device: String,
    previous_focus: usize,
}

pub struct UOpenjevApp {
    pub settings: Option<UOpenjevSettingsDraft>,
    pub expanded: Option<usize>,
    pub pointer: Option<Position>,
    pub drag_editor: Option<usize>,
    pub last_click: Option<(Instant, usize, u16, u16)>,
    pub clipboard: String,
    pub system_clipboard: bool,
    pub backend: Backend,
    pub mouse_regions: Vec<(Rect, EnumOpenjevAction)>,
    pub remote_error: Option<String>,
    pub local_error: Option<String>,
    #[cfg(feature = "local")]
    pub local_config: Option<crate::UOpenjevLocalConfig>,
    #[cfg(feature = "local")]
    pub local_loading: Option<JoinHandle<Result<crate::local::LocalModel>>>,
    pub state: TextArea<'static>,
    pub forms: [Form; 3],
    pub selected: usize,
    pub focus: usize,
    pub model: String,
    pub json_state: bool,
    pub demo: bool,
    pub client: Option<Client>,
    #[cfg(feature = "local")]
    pub local: Option<std::sync::Arc<std::sync::Mutex<crate::local::LocalModel>>>,
    pub local_label: Option<String>,
    pub last: Option<Completed>,
    pub pending: Option<JoinHandle<Result<Completed>>>,
    pub started: Option<Instant>,
    pub status: String,
    pub error: bool,
    pub raw: bool,
    pub scroll: u16,
    pub help: bool,
    pub export_dir: PathBuf,
}
pub type App = UOpenjevApp;

impl App {
    pub fn new(
        model: String,
        state: &str,
        demo: bool,
        client: Option<Client>,
        export_dir: PathBuf,
    ) -> Self {
        Self {
            settings: None,
            expanded: None,
            pointer: None,
            drag_editor: None,
            last_click: None,
            clipboard: String::new(),
            system_clipboard: false,
            backend: if demo { Backend::Demo } else { Backend::Remote },
            mouse_regions: Vec::new(),
            remote_error: None,
            local_error: None,
            #[cfg(feature = "local")]
            local_config: None,
            #[cfg(feature = "local")]
            local_loading: None,
            state: editor(state),
            forms: forms(),
            selected: 0,
            focus: 0,
            model,
            json_state: false,
            demo,
            client,
            last: None,
            pending: None,
            started: None,
            #[cfg(feature = "local")]
            local: None,
            local_label: None,
            status: if demo {
                "DEMO: simulated responses, no network requests"
            } else {
                "Ready. Ctrl+G sends the state to TypeSafe."
            }
            .into(),
            error: false,
            raw: false,
            scroll: 0,
            help: false,
            export_dir,
        }
    }

    pub fn kind(&self) -> Kind {
        Kind::ALL[self.selected]
    }

    pub fn backend_status(&self) -> String {
        match self.backend {
            Backend::Remote => {
                if self.client.is_some() {
                    format!("API ready / {}", self.model)
                } else {
                    "API unavailable / check config".into()
                }
            }
            Backend::Demo => "Simulated responses".into(),
            Backend::Local => {
                #[cfg(feature = "local")]
                {
                    if self.local.is_some() {
                        return self
                            .local_label
                            .clone()
                            .unwrap_or_else(|| "Laya ready".into());
                    }
                    if self.local_loading.is_some() {
                        return "Loading Laya...".into();
                    }
                    if self.local_error.is_some() {
                        return "Laya unavailable / click Local to retry".into();
                    }
                    "Laya not loaded".into()
                }
                #[cfg(not(feature = "local"))]
                {
                    "Requires a local/metal/cuda build".into()
                }
            }
        }
    }

    #[cfg(feature = "local")]
    pub fn start_local_load(&mut self) {
        if self.local.is_some() || self.local_loading.is_some() {
            return;
        }
        let Some(config) = self.local_config.clone() else {
            self.local_error =
                Some("No local checkpoint configured; use --local-model models/laya".into());
            return;
        };
        self.local_error = None;
        self.local_label = Some(format!("Laya / {}", config.device_name()));
        self.local_loading = Some(tokio::task::spawn_blocking(move || config.load()));
    }

    pub fn select_backend(&mut self, backend: Backend) {
        if self.backend == backend && self.pending.is_some() {
            return;
        }
        let cancelled = self.pending.is_some();
        if self.backend != backend {
            self.cancel();
        }
        self.backend = backend;
        self.demo = backend == Backend::Demo;
        #[cfg(feature = "local")]
        if backend == Backend::Local {
            self.start_local_load();
        }
        self.error = false;
        self.status = format!(
            "{} selected. {}{}",
            backend.label(),
            self.backend_status(),
            if cancelled {
                "; previous computation may still finish"
            } else {
                ""
            }
        );
        if backend == Backend::Remote && self.client.is_none() {
            self.fail(anyhow::anyhow!(self.remote_error.clone().unwrap_or_else(
                || "Configure TYPESAFE_TOKEN in the Evo secret file and restart".into()
            )));
        }
        #[cfg(not(feature = "local"))]
        if backend == Backend::Local {
            self.fail(anyhow::anyhow!(
                "Rebuild with --features local, metal, or cuda"
            ));
        }
    }

    pub fn switch_backend(&mut self) {
        self.select_backend(if self.backend == Backend::Remote {
            Backend::Local
        } else {
            Backend::Remote
        });
    }

    pub fn active_editor(&mut self) -> Option<&mut TextArea<'static>> {
        match self.focus {
            0 => Some(&mut self.state),
            1 => Some(&mut self.forms[self.selected].instruction),
            2 => Some(&mut self.forms[self.selected].criteria),
            4 => self.settings.as_mut().map(|draft| &mut draft.model),
            5 => self.settings.as_mut().map(|draft| &mut draft.local_path),
            6 => self.settings.as_mut().map(|draft| &mut draft.export_path),
            _ => None,
        }
    }

    pub fn open_settings(&mut self) {
        if self.settings.is_some() {
            return;
        }
        let (local_path, device): (String, String) = {
            #[cfg(feature = "local")]
            {
                self.local_config
                    .as_ref()
                    .map(|config| {
                        (
                            config.directory.display().to_string(),
                            config.device.clone(),
                        )
                    })
                    .unwrap_or_else(|| ("models/laya".into(), "auto".into()))
            }
            #[cfg(not(feature = "local"))]
            {
                ("models/laya".into(), "auto".into())
            }
        };
        self.settings = Some(UOpenjevSettingsDraft {
            model: editor(&self.model),
            local_path: editor(&local_path),
            export_path: editor(&self.export_dir.display().to_string()),
            device,
            previous_focus: self.focus,
        });
        self.focus = 4;
        self.error = false;
        self.drag_editor = None;
    }

    pub fn close_settings(&mut self) {
        if let Some(draft) = self.settings.take() {
            self.focus = draft.previous_focus;
        }
        self.drag_editor = None;
    }

    pub fn apply_settings(&mut self) -> Result<()> {
        let draft = self.settings.as_ref().context("Settings are not open")?;
        let model = draft.model.lines().join("").trim().to_owned();
        ensure!(
            !model.is_empty() && model.bytes().all(|b| b.is_ascii_graphic()),
            "Remote model must be a nonempty identifier without spaces"
        );
        let export_dir = PathBuf::from(draft.export_path.lines().join(""));
        ensure!(
            export_dir.is_dir(),
            "Export folder must be an existing directory"
        );
        #[cfg(feature = "local")]
        let config = {
            let directory = PathBuf::from(draft.local_path.lines().join(""));
            ensure!(
                !directory.as_os_str().is_empty(),
                "Local checkpoint path must not be empty"
            );
            crate::UOpenjevLocalConfig {
                directory,
                device: draft.device.clone(),
                precision: self
                    .local_config
                    .as_ref()
                    .map_or_else(|| "auto".into(), |config| config.precision.clone()),
            }
        };
        self.model = model;
        self.export_dir = export_dir;
        #[cfg(feature = "local")]
        {
            let changed = self
                .local_config
                .as_ref()
                .is_none_or(|old| old.directory != config.directory || old.device != config.device);
            self.local_config = Some(config);
            if changed {
                if self.backend == Backend::Local {
                    self.cancel();
                }
                if let Some(task) = self.local_loading.take() {
                    task.abort();
                }
                self.local = None;
                self.local_error = None;
                self.local_label = None;
                self.start_local_load();
            }
        }
        self.close_settings();
        self.status =
            "Settings applied for this session. Inputs and completed results are preserved.".into();
        self.error = false;
        Ok(())
    }

    pub fn paste_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = if self.focus >= 4 {
            normalized.replace('\n', " ")
        } else {
            normalized
        };
        if let Some(editor) = self.active_editor() {
            editor.insert_str(text);
        }
    }

    fn write_clipboard(&mut self, text: String) -> Result<()> {
        if self.system_clipboard {
            crate::utility::u_openjev_editor::copy_system(&text)?;
        }
        self.clipboard = text;
        Ok(())
    }

    pub fn edit_action(&mut self, action: EnumOpenjevAction) -> Result<()> {
        use EnumOpenjevAction as Action;
        if action == Action::Copy && self.focus == 3 {
            let result = self.last.as_ref().context("No result to copy")?;
            let text = serde_json::to_string_pretty(&result.response)?;
            self.write_clipboard(text)?;
            self.status = "Result JSON copied.".into();
            self.error = false;
            return Ok(());
        }
        if matches!(action, Action::Copy | Action::Cut) {
            let editor = self.active_editor().context("Select an editable field")?;
            let ((start_row, start_col), (end_row, end_col)) = editor
                .selection_range()
                .context("Select text first, or use Select all")?;
            let mut parts = Vec::new();
            for row in start_row..=end_row {
                let start = if row == start_row { start_col } else { 0 };
                let end = if row == end_row {
                    end_col
                } else {
                    editor.lines()[row].chars().count()
                };
                parts.push(
                    editor.lines()[row]
                        .chars()
                        .skip(start)
                        .take(end - start)
                        .collect::<String>(),
                );
            }
            self.write_clipboard(parts.join("\n"))?;
            if action == Action::Cut {
                self.active_editor().expect("Editor focus retained").cut();
            }
            self.status = if action == Action::Copy {
                "Selection copied."
            } else {
                "Selection cut."
            }
            .into();
        } else if action == Action::Paste {
            ensure!(self.active_editor().is_some(), "Select an editable field");
            let text = if self.system_clipboard {
                crate::utility::u_openjev_editor::paste_system()?
            } else {
                self.clipboard.clone()
            };
            self.paste_text(&text);
            self.status = "Text pasted.".into();
        } else {
            let editor = self.active_editor().context("Select an editable field")?;
            match action {
                Action::Undo => {
                    editor.undo();
                }
                Action::Redo => {
                    editor.redo();
                }
                Action::SelectAll => crate::utility::u_openjev_editor::select_all(editor),
                Action::Clear => {
                    editor.clear();
                }
                _ => {}
            }
        }
        self.error = false;
        Ok(())
    }

    pub fn toggle_expanded(&mut self) {
        if self.expanded.is_some() {
            self.expanded = None;
        } else if self.focus <= 3 {
            self.expanded = Some(self.focus);
        }
        self.drag_editor = None;
    }

    pub fn request(&self) -> Result<Request> {
        let form = &self.forms[self.selected];
        let instructions = Value::String(form.instruction.lines().join("\n"));
        let criteria = form.criteria.lines().join("\n");
        let question = match self.kind() {
            Kind::Choice => Question::Choice {
                instructions,
                criteria: parse_options(&criteria)?,
            },
            Kind::Score => Question::Score {
                instructions,
                criteria: criteria
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| json!(s))
                    .collect(),
            },
            Kind::Noul => {
                let criteria = if criteria.trim().is_empty() {
                    None
                } else {
                    Some(parse_options(&criteria)?)
                };
                Question::Noul {
                    instructions,
                    criteria,
                }
            }
        };
        let state = self.state.lines().join("\n");
        let request = Request {
            state: if self.json_state {
                serde_json::from_str(&state).context("Invalid JSON state")?
            } else {
                json!(state)
            },
            model: self.model.clone(),
            questions: BTreeMap::from([("decision".into(), question)]),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn submit(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let request = match self.request() {
            Ok(request) => request,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        if self.backend == Backend::Remote && self.client.is_none() {
            self.fail(anyhow::anyhow!(
                "Configure TYPESAFE_TOKEN and restart, or use --demo"
            ));
            return;
        }
        if self.backend == Backend::Local {
            #[cfg(feature = "local")]
            if self.local.is_none() {
                self.fail(anyhow::anyhow!(self.local_error.clone().unwrap_or_else(
                    || "Local model is loading; evaluate when it is ready".into()
                )));
                return;
            }
            #[cfg(not(feature = "local"))]
            {
                self.fail(anyhow::anyhow!(
                    "Rebuild with --features local, metal, or cuda"
                ));
                return;
            }
        }
        let client = self.client.clone();
        #[cfg(feature = "local")]
        let local = if self.backend == Backend::Local {
            self.local.clone()
        } else {
            None
        };
        let backend = self.backend;
        let demo = backend == Backend::Demo;
        let start = Instant::now();
        self.started = Some(start);
        self.error = false;
        self.status = "Evaluating... Esc cancels the wait".into();
        self.pending = Some(tokio::spawn(async move {
            #[cfg(feature = "local")]
            if let Some(local) = local {
                return tokio::task::spawn_blocking(move || {
                    let model = local
                        .try_lock()
                        .map_err(|_| anyhow::anyhow!("Local model is still busy; retry when the previous computation finishes"))?;
                    let response = model.evaluate(&request)?;
                    Ok(Completed {
                        backend,
                        ttft_ms: None,
                        first_byte_ms: None,
                        demo: false,
                        elapsed_ms: start.elapsed().as_millis(),
                        request,
                        response,
                    })
                })
                .await?;
            }
            let (response, first_byte_ms) = if demo {
                tokio::time::sleep(Duration::from_millis(350)).await;
                (demo_response(&request), None)
            } else {
                let (response, timing) = client
                    .context("Client not configured")?
                    .evaluate_timed(&request)
                    .await?;
                (response, timing.first_byte_ms)
            };
            Ok(Completed {
                backend,
                ttft_ms: None,
                first_byte_ms,
                demo,
                elapsed_ms: start.elapsed().as_millis(),
                request,
                response,
            })
        }));
    }

    pub async fn collect(&mut self) {
        #[cfg(feature = "local")]
        if self
            .local_loading
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            let task = self.local_loading.take().expect("Finished load exists");
            match task.await {
                Ok(Ok(model)) => {
                    self.local = Some(std::sync::Arc::new(std::sync::Mutex::new(model)));
                    self.local_error = None;
                    if self.backend == Backend::Local {
                        self.status = "Local model ready. Ctrl+G evaluates offline.".into();
                        self.error = false;
                    }
                }
                result => {
                    let message = match result {
                        Ok(Err(error)) => format!("Could not load Laya: {error:#}"),
                        _ => "Local model loading was interrupted".into(),
                    };
                    self.local_error = Some(message.clone());
                    if self.backend == Backend::Local {
                        self.fail(anyhow::anyhow!(message));
                    }
                }
            }
        }
        if self.pending.as_ref().is_some_and(JoinHandle::is_finished) {
            let Some(task) = self.pending.take() else {
                return;
            };
            self.started = None;
            match task.await {
                Ok(Ok(completed)) => {
                    if completed.backend != self.backend {
                        return;
                    }
                    self.status = format!(
                        "{} completed in {} ms. Ctrl+P JSON · Ctrl+O export",
                        completed.backend.label(),
                        completed.elapsed_ms
                    );
                    self.last = Some(completed);
                    self.scroll = 0;
                    self.error = false;
                }
                Ok(Err(e)) => self.fail(e),
                Err(_) => self.fail(anyhow::anyhow!("The request was interrupted")),
            }
        }
    }

    pub fn cancel(&mut self) {
        if let Some(task) = self.pending.take() {
            task.abort();
            self.started = None;
            self.status = "Wait cancelled; local or remote computation may still complete".into();
            self.error = false;
        }
    }

    pub fn fail(&mut self, error: anyhow::Error) {
        self.status = format!("{error:#}");
        self.error = true;
    }

    pub fn export(&mut self) -> Result<PathBuf> {
        let result = self.last.as_ref().context("No response to export")?;
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = self
            .export_dir
            .join(format!("openjev-response-{timestamp}.json"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("Could not create {}", path.display()))?;
        file.write_all(serde_json::to_string_pretty(result)?.as_bytes())?;
        file.write_all(b"\n")?;
        self.error = false;
        self.status = format!("Exported: {}", path.display());
        Ok(path)
    }

    /// Returns true when the user requests exit. Ctrl+C copies; Ctrl+Q quits.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        use EnumOpenjevAction as Action;
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        self.drag_editor = None;
        if control && key.code == KeyCode::Char('q') {
            self.cancel();
            return true;
        }
        if self.help {
            if key.code == KeyCode::Esc || (control && key.code == KeyCode::Char('l')) {
                self.help = false;
            }
            return false;
        }
        let edit = if control {
            match key.code {
                KeyCode::Char('a') => Some(Action::SelectAll),
                KeyCode::Char('c') => Some(Action::Copy),
                KeyCode::Char('x') => Some(Action::Cut),
                KeyCode::Char('v') => Some(Action::Paste),
                KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    Some(Action::Redo)
                }
                KeyCode::Char('z' | 'u') => Some(Action::Undo),
                KeyCode::Char('y' | 'r') => Some(Action::Redo),
                _ => None,
            }
        } else {
            None
        };
        if let Some(action) = edit {
            if let Err(error) = self.edit_action(action) {
                self.fail(error);
            }
            return false;
        }
        if self.settings.is_some() {
            match key.code {
                KeyCode::Esc => self.close_settings(),
                KeyCode::Char('s') if control => {
                    if let Err(error) = self.apply_settings() {
                        self.fail(error);
                    }
                }
                KeyCode::Tab => self.focus = 4 + (self.focus - 4 + 1) % 3,
                KeyCode::BackTab => self.focus = 4 + (self.focus - 4 + 2) % 3,
                KeyCode::Enter | KeyCode::F(_) => {}
                _ if control => {}
                _ => {
                    if let Some(editor) = self.active_editor() {
                        editor.input(key);
                    }
                }
            }
            return false;
        }
        match key.code {
            KeyCode::Char('k') if control => self.open_settings(),
            KeyCode::Char('e') if control => self.toggle_expanded(),
            KeyCode::Char('d') if control => self.switch_backend(),
            KeyCode::Char('l') if control => self.help = true,
            KeyCode::Char('t') if control => self.selected = (self.selected + 1) % 3,
            KeyCode::Char('b') if control => self.json_state = !self.json_state,
            KeyCode::Char('g') if control => self.submit(),
            KeyCode::Char('p') if control => {
                self.raw = !self.raw;
                self.scroll = 0;
            }
            KeyCode::Char('o') if control => {
                if let Err(error) = self.export() {
                    self.fail(error);
                }
            }
            KeyCode::Esc if self.expanded.is_some() => self.expanded = None,
            KeyCode::Esc => self.cancel(),
            KeyCode::Tab | KeyCode::BackTab => {
                let count = 4;
                self.focus = (self.focus
                    + if key.code == KeyCode::Tab {
                        1
                    } else {
                        count - 1
                    })
                    % count;
                if self.expanded.is_some() {
                    self.expanded = Some(self.focus);
                }
            }
            KeyCode::PageDown if self.focus == 3 => self.scroll = self.scroll.saturating_add(8),
            KeyCode::PageUp if self.focus == 3 => self.scroll = self.scroll.saturating_sub(8),
            KeyCode::Down if self.focus == 3 => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up if self.focus == 3 => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Home if self.focus == 3 => self.scroll = 0,
            KeyCode::F(_) => {}
            _ => {
                if let Some(editor) = self.active_editor() {
                    editor.input(key);
                }
            }
        }
        false
    }

    /// Mouse geometry is rebuilt every draw. Drag selection stays in its original editor.
    pub fn mouse(&mut self, event: MouseEvent) -> bool {
        use crate::utility::u_openjev_editor::{place_cursor, select_word};
        use EnumOpenjevAction as Action;
        self.pointer = Some(Position::new(event.column, event.row));
        if event.kind == MouseEventKind::Up(MouseButton::Left) {
            self.drag_editor = None;
            return false;
        }
        if event.kind == MouseEventKind::Drag(MouseButton::Left) && !self.help {
            if let Some(index) = self.drag_editor {
                let rect = self
                    .mouse_regions
                    .iter()
                    .find(|(_, action)| *action == Action::Editor(index))
                    .map(|(rect, _)| *rect);
                if let Some(rect) = rect {
                    self.focus = index;
                    if let Some(editor) = self.active_editor() {
                        if event.row < rect.y {
                            editor.scroll((-1, 0));
                        } else if event.row >= rect.bottom() {
                            editor.scroll((1, 0));
                        }
                        place_cursor(editor, rect, event.column, event.row);
                    }
                }
            }
            return false;
        }
        let Some((rect, action)) = self
            .mouse_regions
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(self.pointer.expect("Pointer recorded")))
            .copied()
        else {
            return false;
        };
        if self.help {
            if event.kind == MouseEventKind::Down(MouseButton::Left) && action == Action::Help {
                self.help = false;
            }
            return false;
        }
        match event.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let delta = if event.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                match action {
                    Action::Editor(index) | Action::Focus(index) => {
                        let previous = self.focus;
                        self.focus = index;
                        if let Some(editor) = self.active_editor() {
                            editor.scroll((delta, 0));
                        }
                        self.focus = previous;
                    }
                    Action::Results => self.scroll = self.scroll.saturating_add_signed(delta),
                    _ => {}
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.drag_editor = None;
                match action {
                    Action::Backend(backend) => self.select_backend(backend),
                    Action::Kind(index) => {
                        self.selected = index;
                        self.last_click = None;
                    }
                    Action::Focus(index) => self.focus = index,
                    Action::Editor(index) => {
                        let double = self.last_click.is_some_and(|(time, field, x, y)| {
                            field == index
                                && x == event.column
                                && y == event.row
                                && time.elapsed() < Duration::from_millis(400)
                        });
                        self.focus = index;
                        if let Some(editor) = self.active_editor() {
                            let extend = event.modifiers.contains(KeyModifiers::SHIFT);
                            if !extend {
                                editor.cancel_selection();
                            } else if !editor.is_selecting() {
                                editor.start_selection();
                            }
                            place_cursor(editor, rect, event.column, event.row);
                            if double && !extend {
                                select_word(editor);
                            } else if !extend {
                                editor.start_selection();
                            }
                        }
                        self.last_click = Some((Instant::now(), index, event.column, event.row));
                        self.drag_editor = Some(index);
                    }
                    Action::Results => self.focus = 3,
                    Action::Evaluate => self.submit(),
                    Action::Cancel => self.cancel(),
                    Action::Preview => {
                        self.raw = !self.raw;
                        self.scroll = 0;
                    }
                    Action::Export => {
                        if let Err(error) = self.export() {
                            self.fail(error);
                        }
                    }
                    Action::Help => self.help = true,
                    Action::StateFormat => self.json_state = !self.json_state,
                    Action::Switch => self.switch_backend(),
                    Action::Settings => self.open_settings(),
                    Action::CloseSettings => self.close_settings(),
                    Action::ApplySettings => {
                        if let Err(error) = self.apply_settings() {
                            self.fail(error);
                        }
                    }
                    Action::Device(device) => {
                        if let Some(draft) = &mut self.settings {
                            draft.device = device.into();
                        }
                    }
                    Action::Expand => self.toggle_expanded(),
                    Action::Copy
                    | Action::Cut
                    | Action::Paste
                    | Action::Undo
                    | Action::Redo
                    | Action::SelectAll
                    | Action::Clear => {
                        if let Err(error) = self.edit_action(action) {
                            self.fail(error);
                        }
                    }
                    Action::Quit => {
                        self.cancel();
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(task) = &self.pending {
            task.abort();
        }
        #[cfg(feature = "local")]
        if let Some(task) = &self.local_loading {
            task.abort();
        }
    }
}

pub fn parse_options(text: &str) -> Result<BTreeMap<String, Value>> {
    let mut options = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let (key, description) = line
            .split_once('=')
            .map_or((line, Value::Null), |(k, v)| (k.trim(), json!(v.trim())));
        ensure!(!key.is_empty(), "An option has an empty key");
        ensure!(
            options.insert(key.to_owned(), description).is_none(),
            "Duplicate option: {key}"
        );
    }
    Ok(options)
}

/// Deterministic UI fixture. Deliberately independent of the input's meaning.
pub fn demo_response(request: &Request) -> Evaluation {
    let answers = request
        .questions
        .iter()
        .map(|(id, q)| {
            let answer = match q {
                Question::Noul { .. } => Answer::Noul { noul: 0.75 },
                Question::Choice { criteria, .. } => {
                    let probabilities = criteria
                        .keys()
                        .enumerate()
                        .map(|(i, k)| {
                            (
                                k.clone(),
                                if i == 0 {
                                    0.8
                                } else {
                                    0.2 / (criteria.len() - 1) as f64
                                },
                            )
                        })
                        .collect();
                    Answer::Choice {
                        choice: criteria.keys().next().cloned().unwrap_or_default(),
                        probabilities,
                        confidence: 0.6,
                    }
                }
                Question::Score { criteria, .. } => {
                    let probabilities = (0..criteria.len())
                        .map(|i| (i.to_string(), 1.0 / criteria.len() as f64))
                        .collect();
                    let legend = criteria
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), v.as_str().unwrap_or("Level").into()))
                        .collect();
                    Answer::Score {
                        score: (criteria.len() - 1) as f64 / 2.0,
                        legend,
                        probabilities,
                        confidence: 0.0,
                    }
                }
            };
            (id.clone(), answer)
        })
        .collect();
    Evaluation {
        model: "demo-fixture (no AI model)".into(),
        answers,
        usage: Usage::default(),
    }
}
