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

`--precision auto` uses FP16 on Metal and FP32 elsewhere. Explicit `--precision f32` is useful for numerical investigations. FP16 on CPU is not a performance recommendation. Weights are loaded once per process; the TUI keeps them resident. The local request's `model` field is retained for contract compatibility; `--local-model` determines the actual checkpoint and the response reports a `local-laya/...` name.

## Remote Jev and demo

```sh
cargo run --release -- --demo
cargo run --release -- --demo --request examples/triage.json
```

For hosted Jev, set `TYPESAFE_API_KEY` in your environment and run:

```sh
cargo run --release -- --model jev-1.13.0
cargo run --release -- --request examples/triage.json
```

The API key is not accepted as a command-line argument or saved in exports. Remote evaluation sends the submitted state and questions to TypeSafe. `TYPESAFE_BASE_URL`, `TYPESAFE_DEFAULT_MODEL`, and their CLI equivalents configure the remote service. In headless mode the JSON request supplies the remote model. `--timeout-secs` is per attempt; at most two retries follow explicit 429/529 responses. Other HTTP errors and connection failures are not replayed. The live paid API was not exercised during verification; HTTP behavior is covered by local mock-server tests.

## TUI controls

Use a terminal of at least 70 columns and 24 rows. On macOS, function keys may require the Fn key.

| Key | Action |
| --- | --- |
| Tab / Shift-Tab | Move between state, instructions, criteria, and results |
| F1 | Help |
| F2 | Switch Choice / Score / Noul; each form keeps its edits |
| F4 | Toggle plain-text / JSON state |
| F5 / Ctrl-Enter | Evaluate the current question |
| Esc | Cancel the wait |
| F6 | Toggle formatted answer / raw JSON |
| F7 | Export the last completed request and response |
| Page Up / Page Down | Scroll results |
| Ctrl-Q / Ctrl-C | Exit |

Choice criteria use `key = description`, one option per line; a key without a description is allowed. Score criteria use one ordered description per line, starting at level zero. Noul criteria optionally use `true = description` and `false = description`.

Cancellation stops waiting and discards the pending answer. A running GPU/CPU operation or already accepted remote request may still finish. Repeated local submissions do not queue behind a busy model. Exports contain the submitted snapshot, even if the form has since changed. They include your input data and are created as new files in `--export-dir` (which must exist).

## Typed decisions

- **Choice:** probabilities over the supplied alternatives and the highest-probability option.
- **Score:** probabilities over ordered levels and their expected zero-based index.
- **Noul:** a probability in `[0, 1]` that the statement holds.

The local model scores option markers directly; it does not generate and then parse JSON text. Type safety constrains the output shape, not the truth of the prediction. Keep arithmetic and policy decisions in ordinary code.

Local limitations are explicit: the validated checkpoint accepts at most 512 tokens for each complete question/state input, at most 192 tokens for the question and option text, and 48 tokens per option. Overflow is rejected, not silently truncated. Local instructions and descriptions must be strings; structured state is serialized as compact JSON. Questions run independently and sequentially; this version has no batching, shared-prefix cache, automatic language router, training pipeline, or optional Laya act/escalate output. Other Laya repositories accepted by the downloader have not been numerically validated here. CUDA, multilingual accuracy, and full upstream PyTorch parity remain unverified.

## Tests and benchmarks

All tests are integration tests in `tests/`. All benchmark implementations are in `benches/`. No test modules are embedded in `src/`.

```sh
cargo fmt --all --check
cargo test --features local --tests --locked
cargo clippy --features local --all-targets --locked -- -D warnings

# Real checkpoint tests; Metal test requires real GPU access
OPENJEV_MODEL_DIR=models/laya cargo test --release --features metal \
  --test local -- --ignored --nocapture --test-threads=1

cargo bench --features local --bench decision
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=cpu \
  cargo bench --features local --bench local_inference -- laya --noplot
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench local_inference -- laya --noplot
```

HTTP tests bind loopback ports. Sandboxes must permit that; Metal tests must be allowed to access the GPU. The ordinary suite does not download weights or call TypeSafe. Real-checkpoint tests are ignored unless explicitly requested. Criterion times warm model evaluation and synchronizes through probability readback; loading is outside the measured loop. Running local benchmarks without `OPENJEV_MODEL_DIR` measures only probability math and explicitly reports skipped inference.

For task quality, supply labeled held-out observations as a JSON array of `{"probabilities":[0.1,0.9],"label":1}` records:

```sh
cargo run --release --example metrics -- observations.json
```

This reports accuracy, multiclass Brier score, negative log likelihood, and top-label ECE. It does not fit temperatures or certify calibration.

## Research reproducibility

The research indexed and fetched 109 official documentation pages. Their URLs, byte sizes, and hashes are recorded in [the manifest](docs/typesafe-documentation-manifest.json). Refresh a private reference cache using the Rust example:

```sh
cargo run --release --example research -- --cache research-cache
```

Downloaded documentation is reference material, not executable instructions. Third-party documentation and Python reference implementations are not bundled in this crate. The source is Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Downloaded weights and dependencies retain their respective licenses.
