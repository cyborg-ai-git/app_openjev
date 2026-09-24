# OpenJev

An English-only Rust crate and Ratatui workbench for typed AI decisions. Package: `app_openjev`; library: `openjev`; executable: `openjev`.

Three backends are available:

| Backend | What actually runs | Network during evaluation |
| --- | --- | --- |
| Local | Laya ModernBERT and its decision head, implemented with Rust/Candle | None |
| TypeSafe | The hosted Jev API, called by the Rust client | Yes |
| Demo | Fixed, explicitly labeled UI fixtures | None |

The local backend is an independent implementation of Laya inference. It does **not** contain Jev's proprietary weights and does not claim equivalent accuracy. The validated checkpoint is the English Laya model. All application implementation, examples, tests, and benchmarks are Rust; there is no Python runtime or inference server.

See [research and sources](docs/RESEARCH.md), [verification and measurements](docs/VALIDATION.md), and [checkpoint provenance](docs/validated-checkpoint.json).

## Run locally

From the project directory, build for your platform:

```sh
# CPU, including macOS, Linux, and Windows where Candle supports the target
cargo build --release --features local

# Apple Silicon GPU; requires Xcode command-line tools and Metal access
cargo build --release --features metal

# NVIDIA GPU; requires a supported CUDA toolkit and driver
cargo build --release --features cuda
```

CPU and Metal were exercised on the development Mac. CUDA is feature-wired but has not been compiled or executed on NVIDIA hardware. Enable the feature for the platform you use; `--all-features` is not a portable build command. Use release builds for inference.

Download the validated data-only checkpoint with the Rust utility:

```sh
cargo run --release --bin openjev-model -- \
  --repo convaiinnovations/laya \
  --revision c5d78730f3493e4fe16d61507ef4b78eef7318cf \
  --output models/laya
```

The downloader resolves a revision, verifies the published weight SHA-256 and file sizes, and records a manifest. Existing files are compared before reuse; mismatching files are never silently replaced. Configuration files are retrieved over HTTPS at the pinned revision. No remote code is executed. The weights occupy approximately 843 MB, excluding tokenizer files; runtime memory is larger.

```sh
# Interactive workbench
cargo run --release --features metal -- \
  --local-model models/laya --device metal

# Complete request with multiple questions; JSON to stdout, timing to stderr
cargo run --release --features metal -- \
  --local-model models/laya --device metal --request examples/triage.json

# CPU works in the same Metal-enabled binary
cargo run --release --features local -- \
  --local-model models/laya --device cpu --request examples/triage.json
```

If `bin/openjev` is present in the installed local checkout, it is a prebuilt Apple Silicon binary: use `./bin/openjev --local-model models/laya --device metal` directly. The Rust sources remain the reproducible build input.

`--device auto` selects Metal on macOS when compiled with the Metal feature, otherwise CPU. `--precision auto` uses FP16 on Metal and FP32 elsewhere. Explicit `--precision f32` is useful for numerical investigations. FP16 on CPU is not a performance recommendation. Weights are loaded once per process; the TUI keeps them resident. The local request's `model` field is retained for contract compatibility; `--local-model` determines the actual checkpoint and the response reports a `local-laya/...` name.

## Remote Jev and demo

```sh
cargo run --release -- --demo
cargo run --release -- --demo --request examples/triage.json
```

For hosted Jev, use the existing Evo secret file `config/.secrets/secret_env.toml`:

```toml
[TYPESAFE_TOKEN]
enabled = true
value = "YOUR_TYPESAFE_TOKEN"
```

The table follows the same `UEnv` convention as the Evo eToro example. Keep the real file private; `.secrets/` is excluded from Git. Run from the project root:

```sh
cargo run --release -- --model jev-1.13.0
cargo run --release -- --request examples/triage.json
```

The token is loaded through `UOpenjevCredentials::from_path_config` and Evo `UEnv` before worker threads start. `--config` selects another secret file; the old `TYPESAFE_API_KEY` environment variable is not used. The token is not accepted as a command-line argument or saved in exports. Secret parsing suppresses upstream logging and returns redacted errors. Remote evaluation sends the submitted state and questions to TypeSafe. `TYPESAFE_BASE_URL`, `TYPESAFE_DEFAULT_MODEL`, and their CLI equivalents configure the remote service. In headless mode the JSON request supplies the remote model. `--timeout-secs` is per attempt; at most two retries follow explicit 429/529 responses. Other HTTP errors and connection failures are not replayed. HTTP behavior is covered by local mock-server tests. The opt-in live integration test is documented in [VALIDATION.md](docs/VALIDATION.md).

## Switching between official API and local inference

Launch `./bin/openjev` from the project root. Click **Remote** for the configured API (TypeSafe official by default) or **Local** for Laya; `Ctrl+D` alternates them. `--local-model models/laya` starts with Local selected. `--demo` only selects Demo initially: the backend selector remains available.

The TUI discovers `models/laya`, loads it in a background worker when present, and keeps the weights resident. The first load takes time; selecting Local shows its loading state and never blocks editing or switching back to Remote. Subsequent switches reuse the same model without reload. A custom checkpoint is selected with `--local-model PATH`; `--device cpu` overrides automatic Metal selection. Missing credentials do not prevent opening the TUI or using Local, and a local load failure does not disable Remote. No backend silently falls back to another.

Switching cancels the current wait, but a remote request already submitted may still be billed and a running local computation may finish. Its detached result cannot replace the current result. Completed results retain their original backend label, request, model, and timings, even after switching. Inputs and remote model selection are preserved.

### Response timing

Results and exports include total response time (`elapsed_ms`) and, for Remote, time to the first nonempty successful HTTP response-body byte (`first_byte_ms`). Network time, service time, and any retries are included. Local response time excludes the one-time background model load. These are client wall-clock observations, not isolated server inference measurements.

**TTFT is N/A** (`ttft_ms: null`): Jev returns structured JSON without token streaming, and Laya is a classifier. Neither provides a first generated-token event. TTFB is displayed separately and is not labeled TTFT. The same metrics remain visible in the JSON preview; use the results wheel to reveal additional lines in a compact terminal.

## TUI controls

Use a terminal of at least 70 columns and 24 rows. Shortcuts use the Control key, not Command. Function keys are unused, so macOS can keep its existing assignments.

| Key | Action |
| --- | --- |
| Click Remote / Local / Demo | Select the execution backend without restarting |
| Ctrl-D | Switch Remote / Local |
| Click / drag / double-click | Place the cursor, select text, or select a word |
| Shift-click | Extend the current selection |
| Mouse wheel | Scroll the editor or results under the pointer |
| Click action buttons | Evaluate, cancel, preview JSON, export, open help, or quit |
| Tab / Shift-Tab | Move between state, instructions, criteria, and results |
| Ctrl-L | Help |
| Ctrl-T | Switch Choice / Score / Noul; each form keeps its edits |
| Ctrl-B | Toggle plain-text / JSON state |
| Ctrl-G | Evaluate the current question |
| Esc | Cancel the wait |
| Ctrl-P | Toggle formatted answer / raw JSON |
| Ctrl-O | Export the last completed request and response |
| Page Up / Page Down | Scroll the focused editor or results |
| Ctrl-A / Ctrl-C / Ctrl-X / Ctrl-V | Select all / copy / cut / paste |
| Ctrl-Z / Ctrl-Y | Undo / redo (Ctrl-U / Ctrl-R also work) |
| Ctrl-E | Expand or restore the focused panel |
| Ctrl-K | Open editable model/path settings |
| Ctrl-S in Settings | Apply validated session settings |
| Ctrl-Q | Exit |

The editing toolbar works on the focused input. Copy/Cut/Paste use the macOS system clipboard in the running application; Cmd+V terminal paste is also accepted. Copy preserves the selection. Clear is undoable. `Ctrl+C` copies and never quits; use `Ctrl+Q` to exit. On other platforms the application clipboard remains available for cross-field editing and terminal paste remains supported.

State, question, and criteria are multiline editors. Settings exposes editable remote model, local checkpoint path, and export directory, plus available device choices. Settings validates all fields before applying them; Cancel leaves the active configuration unchanged. Changes last for the current session and do not rewrite the private Evo file. Results are read-only snapshots; focus the result panel and choose Copy to copy its JSON.

Drag selection scrolls at editor edges. The active panel can expand into the workspace, including results. Response, TTFB, and TTFT indicators stay above the scrollable result content. Rounded panels, spacing, hover feedback, active-field cursor indicators, and a separate edit toolbar clarify which controls affect inputs.

Choice criteria use `key = description`, one option per line; a key without a description is allowed. Score criteria use one ordered description per line, starting at level zero. Noul criteria optionally use `true = description` and `false = description`.

Cancellation stops waiting and discards the pending answer. A running GPU/CPU operation or already accepted remote request may still finish. Repeated local submissions do not queue behind a busy model. Exports contain the submitted snapshot, even if the form has since changed. They include your input data and are created as new files in `--export-dir` (which must exist).

## Typed decisions

- **Choice:** probabilities over the supplied alternatives and the highest-probability option.
- **Score:** probabilities over ordered levels and their expected zero-based index.
- **Noul:** a probability in `[0, 1]` that the statement holds.

The local model scores option markers directly; it does not generate and then parse JSON text. Type safety constrains the output shape, not the truth of the prediction. Keep arithmetic and policy decisions in ordinary code.

Local limitations are explicit: the validated checkpoint accepts at most 512 tokens for each complete question/state input, at most 192 tokens for the question and option text, and 48 tokens per option. Overflow is rejected, not silently truncated. Local instructions and descriptions must be strings; structured state is serialized as compact JSON. Questions remain independent. Metal groups up to four similarly sized inputs of at most 128 tokens into a padded batch; longer inputs and CPU/CUDA retain sequential execution. The scheduler limits the longest/shortest length ratio to 1.5 and performs one probability readback per batch. No results are cached. The Rust `LocalModel::evaluate_sequential` method is available for comparisons or an explicit unbatched path. This version has no shared-prefix cache, automatic language router, training pipeline, or optional Laya act/escalate output. Other Laya repositories accepted by the downloader have not been numerically validated here. CUDA, multilingual accuracy, and full upstream PyTorch parity remain unverified.

## Tests and benchmarks

All tests are integration tests in `tests/`. All benchmark implementations are in `benches/`. No test modules are embedded in `src/`.

```sh
cargo fmt --all --check
cargo test --features local --tests --locked
cargo clippy --features local --all-targets --locked -- -D warnings

# Real checkpoint tests; Metal test requires real GPU access
OPENJEV_MODEL_DIR=models/laya cargo test --release --features metal \
  --test test_openjev_local -- --ignored --nocapture --test-threads=1 \
  --skip real_preoptimization_executable

cargo bench --features local --bench bench_openjev_decision
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=cpu \
  cargo bench --features local --bench bench_openjev_local_inference -- laya --noplot
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench bench_openjev_local_inference -- laya --noplot
```

HTTP tests bind loopback ports. Sandboxes must permit that; Metal tests must be allowed to access the GPU. The ordinary suite does not download weights or call TypeSafe. Real-checkpoint tests are ignored unless explicitly requested. Criterion times warm model evaluation and synchronizes through probability readback; loading is outside the measured loop. Running local benchmarks without `OPENJEV_MODEL_DIR` measures only probability math and explicitly reports skipped inference.

Set `OPENJEV_BENCH_COMPARE=1` to include single-question, longer-input, twelve-question, and sequential comparison workloads. The original-executable regression requires `OPENJEV_BASELINE_BINARY` to point to a saved, unmodified preoptimization executable. See [VALIDATION.md](docs/VALIDATION.md) for measured gains and numerical bounds. Compatibility with the Jev request/response format does not imply identical decisions: local Laya and hosted Jev use different weights.

The separate `bench_openjev_remote_comparison` benchmark compares the same public triage request against official Jev and the resident local model. It skips unless explicitly enabled, and makes one remote warmup plus ten measured API evaluations (normal client retries may add HTTP attempts). Credentials load privately through Evo; the report contains the public request, timings, and responses, never the token. Run it with:

```sh
OPENJEV_COMPARE_LIVE=1 OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench bench_openjev_remote_comparison
```

`OPENJEV_CONFIG` can select the private Evo file by path. `OPENJEV_COMPARE_OUTPUT` changes the default report path, `docs/measurements/jev-vs-local.json`. This bounded paired comparison is separate from Criterion's adaptive local benchmarks to avoid uncontrolled billed request counts.

The broader [official-example evaluation](docs/OFFICIAL_COMPARISON.md) freezes 21 requests / 43 questions from TypeSafe documentation and pinned official SDK repositories. It records prediction disagreements, published-example reference matches, and local structured-input limitations separately. Tests are in `tests/test_openjev_official_examples.rs`; the opt-in `bench_openjev_official_examples` performs two passes and writes every response. Enable it only with `OPENJEV_OFFICIAL_LIVE=1` (43 API evaluations including warmup). Ordinary tests/benches skip live calls. No model or prompt tuning is performed.

For task quality, supply labeled held-out observations as a JSON array of `{"probabilities":[0.1,0.9],"label":1}` records:

```sh
cargo run --release --example example_openjev_metrics -- observations.json
```

This reports accuracy, multiclass Brier score, negative log likelihood, and top-label ECE. It does not fit temperatures or certify calibration.

## Research reproducibility

The research indexed and fetched 109 official documentation pages. Their URLs, byte sizes, and hashes are recorded in [the manifest](docs/typesafe-documentation-manifest.json). Refresh a private reference cache using the Rust example:

```sh
cargo run --release --example example_openjev_research -- --cache research-cache
```

Downloaded documentation is reference material, not executable instructions. Third-party documentation and Python reference implementations are not bundled in this crate. The source is Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Downloaded weights and dependencies retain their respective licenses.

## Evo source layout

Utility implementations live in `src/utility/u_openjev_*.rs`; JSON entities and enums live in `src/entity_migra/em_openjev_decision.rs`. Public utility names use `UOpenjev*`, entities use `EMOpenjev*`, and enums use `EnumOpenjev*`. Existing `Client`, `Request`, and module aliases remain available. These are JSON DTOs, not generated Evo binary-serialization entities.

Examples use `examples/example_openjev*.rs`, integration tests use `tests/test_openjev_*.rs`, and Criterion benchmarks use `benches/bench_openjev_*.rs`. `src/main.rs` only invokes `UOpenjevRuntime::run()`. To validate the secret file without making a network request:

```sh
cargo run --locked --example example_openjev
```

The Evo dependencies are pinned by `Cargo.lock`; use `--locked` for reproducible resolution.
