use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{Answer, Client, Evaluation, Question, Request, Usage};
use anyhow::{Context, Result, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
    pub demo: bool,
    pub elapsed_ms: u128,
    pub request: Request,
    pub response: Evaluation,
}

pub struct App {
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

impl App {
    pub fn new(
        model: String,
        state: &str,
        demo: bool,
        client: Option<Client>,
        export_dir: PathBuf,
    ) -> Self {
        Self {
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
                "Ready. F5 sends the state to TypeSafe."
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

    pub fn active_editor(&mut self) -> Option<&mut TextArea<'static>> {
        match self.focus {
            0 => Some(&mut self.state),
            1 => Some(&mut self.forms[self.selected].instruction),
            2 => Some(&mut self.forms[self.selected].criteria),
            _ => None,
        }
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
        if !self.demo && self.client.is_none() && self.local_label.is_none() {
            self.fail(anyhow::anyhow!(
                "Set TYPESAFE_API_KEY and restart, or use --demo"
            ));
            return;
        }
        let client = self.client.clone();
        #[cfg(feature = "local")]
        let local = self.local.clone();
        let demo = self.demo;
        self.started = Some(Instant::now());
        self.error = false;
        self.status = "Evaluating... Esc cancels the wait".into();
        self.pending = Some(tokio::spawn(async move {
            let start = Instant::now();
            #[cfg(feature = "local")]
            if let Some(local) = local {
                return tokio::task::spawn_blocking(move || {
                    let model = local
                        .try_lock()
                        .map_err(|_| anyhow::anyhow!("Local model is still busy; retry when the previous computation finishes"))?;
                    let response = model.evaluate(&request)?;
                    Ok(Completed {
                        demo: false,
                        elapsed_ms: start.elapsed().as_millis(),
                        request,
                        response,
                    })
                })
                .await?;
            }
            let response = if demo {
                tokio::time::sleep(Duration::from_millis(350)).await;
                demo_response(&request)
            } else {
                client
                    .context("Client not configured")?
                    .evaluate(&request)
                    .await?
            };
            Ok(Completed {
                demo,
                elapsed_ms: start.elapsed().as_millis(),
                request,
                response,
            })
        }));
    }

    pub async fn collect(&mut self) {
        if self.pending.as_ref().is_some_and(JoinHandle::is_finished) {
            let Some(task) = self.pending.take() else {
                return;
            };
            self.started = None;
            match task.await {
                Ok(Ok(completed)) => {
                    self.status = format!(
                        "{}Completed in {} ms. F6 JSON · F7 export",
                        if completed.demo { "DEMO / " } else { "" },
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

    /// Returns true when the user requests exit.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q' | 'c'))
        {
            self.cancel();
            return true;
        }
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
                self.help = false;
            }
            return false;
        }
        match key.code {
            KeyCode::F(1) => self.help = true,
            KeyCode::F(2) => {
                self.selected = (self.selected + 1) % 3;
            }
            KeyCode::F(4) => {
                self.json_state = !self.json_state;
            }
            KeyCode::F(5) => self.submit(),
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => self.submit(),
            KeyCode::F(6) => {
                self.raw = !self.raw;
                self.scroll = 0;
            }
            KeyCode::F(7) => {
                if let Err(e) = self.export() {
                    self.fail(e);
                }
            }
            KeyCode::Esc => self.cancel(),
            KeyCode::Tab => self.focus = (self.focus + 1) % 4,
            KeyCode::BackTab => self.focus = (self.focus + 3) % 4,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(8),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
            KeyCode::Down if self.focus == 3 => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up if self.focus == 3 => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Home if self.focus == 3 => self.scroll = 0,
            _ => {
                if let Some(editor) = self.active_editor() {
                    editor.input(key);
                }
            }
        }
        false
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(task) = &self.pending {
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
