use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyEventKind},
    execute,
};
use futures_util::StreamExt;
use openjev::{Client, app, ui};

#[derive(Parser)]
#[command(
    version,
    about = "OpenJev: Ratatui workbench for TypeSafe AI / Jev",
    long_about = "Edit Choice, Score, and Noul decisions. Set TYPESAFE_API_KEY for the remote API, use --demo for simulated data, or --local-model for offline Laya inference."
)]
struct Args {
    /// Offline simulation with fixed responses (no AI model)
    #[arg(long)]
    demo: bool,
    /// Original Laya checkpoint directory; offline Rust/Candle inference
    #[arg(long, conflicts_with = "demo")]
    local_model: Option<PathBuf>,
    #[arg(long, default_value = "cpu", value_parser = ["cpu", "metal", "cuda"])]
    device: String,
    /// auto uses f16 on Metal and f32 elsewhere
    #[arg(long, default_value = "auto", value_parser = ["auto", "f32", "f16"])]
    precision: String,
    /// Evaluate a complete JSON request without the TUI; print the JSON response
    #[arg(long)]
    request: Option<PathBuf>,
    #[arg(long, env = "TYPESAFE_DEFAULT_MODEL", default_value = "jev-latest")]
    model: String,
    #[arg(
        long,
        env = "TYPESAFE_BASE_URL",
        default_value = "https://api.typesafe.ai"
    )]
    base_url: String,
    /// Read the initial state from a UTF-8 file
    #[arg(long)]
    state_file: Option<PathBuf>,
    /// Interpret state as JSON instead of plain text
    #[arg(long)]
    json_state: bool,
    #[arg(long, default_value = ".")]
    export_dir: PathBuf,
    /// Timeout per HTTP attempt; 429/529 retries may extend the total wait
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=300))]
    timeout_secs: u64,
}

struct PasteGuard;
impl Drop for PasteGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.request.is_none() {
        ensure!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "Run openjev in an interactive terminal (or use --help or --request)"
        );
    }
    let state = match args.state_file {
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?,
        None => "I was charged twice for the same order. Can you fix this today?".into(),
    };
    let client = if args.demo || args.local_model.is_some() {
        None
    } else {
        std::env::var("TYPESAFE_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|key| Client::new(&key, &args.base_url, Duration::from_secs(args.timeout_secs)))
            .transpose()?
    };
    let mut app = app::App::new(args.model, &state, args.demo, client, args.export_dir);
    app.json_state = args.json_state;
    if let Some(path) = args.local_model {
        #[cfg(feature = "local")]
        {
            eprintln!("Loading Laya from {} on {}...", path.display(), args.device);
            let device = openjev::local::device(&args.device)?;
            let model = if args.precision == "auto" {
                openjev::local::LocalModel::load(&path, device)?
            } else {
                openjev::local::LocalModel::load_with_options(
                    &path,
                    device,
                    openjev::local::LoadOptions {
                        dtype: if args.precision == "f16" {
                            candle_core::DType::F16
                        } else {
                            candle_core::DType::F32
                        },
                        reference_encoder: false,
                    },
                )?
            };
            app.local = Some(std::sync::Arc::new(std::sync::Mutex::new(model)));
            app.local_label = Some(format!("LOCAL · Laya / {}", args.device));
            app.model = "Laya (experimental runtime)".into();
            app.status = "Ready. F5 evaluates offline with Laya. F1 help.".into();
        }
        #[cfg(not(feature = "local"))]
        {
            let _ = path;
            anyhow::bail!("Rebuild with --features local (CPU), metal, or cuda");
        }
    }
    if let Some(path) = args.request {
        let request: openjev::Request = serde_json::from_slice(&std::fs::read(path)?)?;
        request.validate()?;
        let start = std::time::Instant::now();
        #[cfg(feature = "local")]
        if let Some(local) = &app.local {
            let response = local
                .lock()
                .map_err(|_| anyhow::anyhow!("Model unavailable"))?
                .evaluate(&request)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            eprintln!("Local inference: {} ms", start.elapsed().as_millis());
            return Ok(());
        }
        let response = if app.demo {
            app::demo_response(&request)
        } else {
            app.client
                .as_ref()
                .context("Set TYPESAFE_API_KEY or use --demo")?
                .evaluate(&request)
                .await?
        };
        println!("{}", serde_json::to_string_pretty(&response)?);
        eprintln!("Evaluation: {} ms", start.elapsed().as_millis());
        return Ok(());
    }
    let mut terminal = ratatui::try_init()?;
    let result = run(&mut terminal, &mut app).await;
    ratatui::restore();
    result
}

async fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut app::App) -> Result<()> {
    execute!(io::stdout(), EnableBracketedPaste)?;
    let _paste = PasteGuard;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(80));
    loop {
        app.collect().await;
        terminal.draw(|frame| ui::draw(frame, app))?;
        tokio::select! {
            _ = tick.tick() => {},
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => {
                    if app.key(key) { break; }
                },
                Some(Ok(Event::Paste(text))) if !app.help => {
                    if let Some(editor) = app.active_editor() { editor.insert_str(text.replace("\r\n", "\n").replace('\r', "\n")); }
                },
                Some(Err(e)) => return Err(e.into()),
                None => break,
                _ => {},
            }
        }
    }
    Ok(())
}
