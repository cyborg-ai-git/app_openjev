use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

use crate::{Client, UOpenjevCredentials, app, ui};
use anyhow::{Context, Result, ensure};
use clap::Parser;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyEventKind,
    },
    execute,
};
use futures_util::StreamExt;

#[derive(Parser)]
#[command(
    version,
    about = "OpenJev: Ratatui workbench for TypeSafe AI / Jev",
    long_about = "Edit Choice, Score, and Noul decisions. Configure TYPESAFE_TOKEN in config/.secrets/secret_env.toml for the remote API, use --demo for simulated data, or --local-model for offline Laya inference."
)]
struct Args {
    /// Evo secret configuration containing the enabled TYPESAFE_TOKEN table
    #[arg(long, default_value = "./config/.secrets/secret_env.toml")]
    config: PathBuf,
    /// Offline simulation with fixed responses (no AI model)
    #[arg(long)]
    demo: bool,
    /// Original Laya checkpoint directory; offline Rust/Candle inference
    #[arg(long, conflicts_with = "demo")]
    local_model: Option<PathBuf>,
    #[arg(long, default_value = "auto", value_parser = ["auto", "cpu", "metal", "cuda"])]
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
        let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    }
}

/// Application entry point; load Evo secrets before starting worker threads.
pub struct UOpenjevRuntime;
impl UOpenjevRuntime {
    pub fn run() -> Result<()> {
        let args = Args::parse();
        if args.request.is_none() {
            ensure!(
                io::stdin().is_terminal() && io::stdout().is_terminal(),
                "Run openjev in an interactive terminal (or use --help or --request)"
            );
        }
        let remote = if args.request.is_some() && (args.demo || args.local_model.is_some()) {
            Ok(None)
        } else {
            load_remote_client(&args).map(Some)
        };
        let (client, remote_error) = match remote {
            Ok(client) => (client, None),
            Err(error) if args.request.is_none() => (None, Some(error.to_string())),
            Err(error) => return Err(error),
        };
        let runtime = tokio::runtime::Runtime::new()?;
        let result = runtime.block_on(run_application(args, client, remote_error));
        // Detached blocking inference/loading must not hold the terminal open on quit.
        runtime.shutdown_timeout(Duration::from_millis(100));
        result
    }
}

fn load_remote_client(args: &Args) -> Result<Client> {
    let credentials = UOpenjevCredentials::from_path_config(
        args.config.to_str().context("The configuration path must be UTF-8")?,
    ).map_err(|_| anyhow::anyhow!(
        "Could not load enabled TYPESAFE_TOKEN from the Evo secret configuration; check --config, TOML syntax, and the value field"
    ))?;
    credentials.client(&args.base_url, Duration::from_secs(args.timeout_secs))
}

async fn run_application(
    args: Args,
    client: Option<Client>,
    remote_error: Option<String>,
) -> Result<()> {
    let state = match args.state_file {
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?,
        None => "I was charged twice for the same order. Can you fix this today?".into(),
    };
    let mut app = app::App::new(args.model, &state, args.demo, client, args.export_dir);
    app.system_clipboard = cfg!(target_os = "macos");
    app.json_state = args.json_state;
    app.remote_error = remote_error;
    let start_local = args.local_model.is_some();
    #[cfg(feature = "local")]
    {
        let config = crate::UOpenjevLocalConfig {
            directory: args
                .local_model
                .clone()
                .unwrap_or_else(|| PathBuf::from("models/laya")),
            device: args.device,
            precision: args.precision,
        };
        app.local_config = Some(config.clone());
        if args.request.is_some() && start_local {
            let model = config.load()?;
            app.local = Some(std::sync::Arc::new(std::sync::Mutex::new(model)));
        } else if args.request.is_none() && (start_local || config.directory.is_dir()) {
            app.start_local_load();
        }
    }
    #[cfg(not(feature = "local"))]
    if start_local && args.request.is_some() {
        anyhow::bail!("Rebuild with --features local (CPU), metal, or cuda");
    }
    if args.request.is_none() {
        let backend = if start_local {
            crate::EnumOpenjevBackend::Local
        } else if args.demo {
            crate::EnumOpenjevBackend::Demo
        } else {
            crate::EnumOpenjevBackend::Remote
        };
        app.select_backend(backend);
    }
    if let Some(path) = args.request {
        let request: crate::Request = serde_json::from_slice(&std::fs::read(path)?)?;
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
                .context("Configure TYPESAFE_TOKEN in the Evo secret configuration or use --demo")?
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
    let _paste = PasteGuard;
    execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
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
                Some(Ok(Event::Mouse(event))) => { if app.mouse(event) { break; } },
                Some(Ok(Event::Paste(text))) if !app.help => {
                    app.paste_text(&text);
                },
                Some(Err(e)) => return Err(e.into()),
                None => break,
                _ => {},
            }
        }
    }
    Ok(())
}
