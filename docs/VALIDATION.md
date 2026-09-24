# Verification and measured performance

Date: 2026-09-21. These results describe this checkout and the pinned English Laya checkpoint, not Jev's hosted performance or a claim of production readiness.

## Environment and artifacts

| Item | Value |
| --- | --- |
| Machine | Apple M5, Mac17,2 |
| Memory | 32 GB unified memory |
| Operating system | macOS 27.0, build 26A428 |
| Rust | rustc 1.96.0 (`ac68faa20`, 2026-05-25) |
| Candle | 0.9.2, pinned in Cargo.lock |
| Model | `convaiinnovations/laya` |
| Revision | `c5d78730f3493e4fe16d61507ef4b78eef7318cf` |
| Weights SHA-256 | `891102d372688fc2a094dac56a384bc537b87c63f21f9f3dac0be2b7cbc8d86c` |
| Weights size | 842,609,210 bytes |
| CPU precision | FP32 |
| Metal precision | FP16; rotary angles computed in FP32 before conversion |

All five downloaded artifacts and their hashes are listed in [validated-checkpoint.json](validated-checkpoint.json). The downloader was also rerun on the existing directory and successfully verified/reused the same files.

## Automated checks

The current Metal-enabled integration suite has **41 ordinary tests**, all passing, and the API-only suite has **36**. The new coverage includes the official-example validation and comparison-metric tests. Six additional real-checkpoint/residency tests, one live API test, and one visual fixture exporter are ignored by default. The broader official prediction comparison and its capability gaps are recorded in [OFFICIAL_COMPARISON.md](OFFICIAL_COMPARISON.md). All test implementations are in `tests/`; source files contain no test modules.

| Area | Checks |
| --- | --- |
| Contracts | Three primitive round trips; invalid/empty state; duplicate options; mismatched response IDs/types; nonfinite/out-of-range probabilities; invalid option distributions |
| Evo credentials and startup | Standard enabled/value tables; invalid, missing, disabled, oversized and malformed files; no stale token reuse; redacted Debug/errors; trace-log protection; correct bearer header; executable default-path discovery; legacy environment key ignored; demo works without credentials |
| HTTP | Actual loopback server; correct POST body and bearer header; no retry on unauthorized replies; bounded rate-limit retries; timeout and malformed JSON; HTTPS enforcement; oversized response rejection; no echo of malformed server data |
| TUI | All primitives at multiple terminal dimensions; Unicode; raw JSON and help; extreme scroll offsets; retained edits; cancellation; exports linked to the submitted snapshot; visible inputs at 70×24; Control shortcuts; function keys ignored |
| Local preprocessing | Stable softmax; Noul ordering; Score expectation; temperature buckets; marker sanitization; overflow rejection |
| Encoder | Deterministic synthetic weights with local/global attention, compared against Candle's ModernBERT reference at multiple sequence lengths |
| Metrics | Perfect predictions; calibrated uniform predictions; confidently wrong predictions; invalid data |
| Real checkpoint | All three primitives; independent-question agreement; native CPU and Metal probabilities compared with the reference encoder |

`cargo fmt --all --check` and `cargo clippy --features metal --all-targets --locked --offline -- -D warnings` pass. The default API/demo feature configuration was also tested successfully. Release CPU/Metal inference and the model downloader were exercised as executables. An interactive PTY smoke check opened the TUI, evaluated the demo, exited with Ctrl-Q, and confirmed that the original terminal settings were restored. When GPU access is unavailable in the sandbox, Metal startup returns an explanatory error rather than panicking.

The installed local TUI was also exercised with real Metal weights. Two consecutive evaluations of its default single Choice question completed in 52 ms and 44 ms, respectively, through the background worker. These are smoke observations, not Criterion measurements; the three-question benchmark below uses a different workload. The subsequent shortcut update replaces function keys with Control shortcuts; the README and in-app help contain the current bindings.

The real parity check uses four English inputs and three questions per input. The maximum absolute probability differences against the full decision pipeline using Candle's FP32 reference encoder were:

| Compared backend | Maximum absolute probability difference | Test tolerance |
| --- | --- | --- |
| Native CPU FP32 | 0.00000308 | less than 0.0001 |
| Native Metal FP16 | 0.00165898 | less than 0.02 |

This is a small numerical regression check. The reference and optimized pipelines share the Rust decision-head implementation, so agreement does not independently validate that head against PyTorch. It also does not measure classification accuracy, prove calibration, or guarantee the same error bound on every input.

## Original warm inference baseline

Workload: [examples/triage.json](../examples/triage.json), one shared short state and three questions, covering Choice, Noul, and Score. The original engine evaluated those questions sequentially. These historical measurements precede the adaptive batching update recorded below. Criterion loads the model outside the timing loop, warms for two seconds, requests ten samples over at least ten seconds, and includes tokenization and probability readback inside the timed evaluation. CPU and Metal runs were performed separately.

| Backend | Mean per complete three-question request | 95% confidence interval for the mean |
| --- | --- | --- |
| CPU FP32 | 527.36 ms | 523.06–532.82 ms |
| Metal FP16 | 79.42 ms | 78.65–80.36 ms |

The measured mean ratio is approximately 6.64× on this workload. This comparison includes a precision difference; it is not an isolated test of GPU hardware alone. Criterion's displayed Metal slope estimate was 79.30 ms; the table consistently uses the sample mean from its JSON artifacts. The CPU run reported one mild high outlier. Ten samples on one machine do not establish a service-level p95 or p99 latency.

Fresh-process CPU smoke runs observed approximately 524–526 ms for the first evaluation after loading. Metal smoke runs varied from 121 ms to 4,932 ms, including a slower first invocation of the installed executable. These individual observations include first-use effects and system variability; their cause was not isolated, and they are not a cold-start distribution. Weight-loading time is excluded from the CLI's `Local inference` measurement. Do not interpret the warm benchmark as a startup guarantee. Repeated CLI invocations reload weights. The TUI keeps them resident.

Raw Criterion estimates, in nanoseconds, are retained in [measurements/laya-cpu.json](measurements/laya-cpu.json) and [measurements/laya-metal.json](measurements/laya-metal.json). Final executable responses are in [cpu-response.json](measurements/cpu-response.json) and [metal-response.json](measurements/metal-response.json).

## Application overhead

Criterion microbenchmarks used ten samples, one-second warmup, and one-second measurement windows. These are short fixture measurements, not general limits.

| Operation | Criterion displayed point estimate |
| --- | --- |
| Validate the three-question request | 35.45 ns |
| Serialize the request | 388.08 ns |
| Validate the response | 161.69 ns |
| Render the 120×36 TUI with TestBackend | 81.81 µs |

The TUI benchmark measures an in-memory backend, not terminal I/O or OS rendering. It uses the wide layout, whose behavior is unchanged by the later compact-layout visibility fix. Raw estimates are retained in `measurements/`. Both benchmark implementations live exclusively in `benches/`.

## Reproduce

Run these commands from the project root after downloading the pinned checkpoint to `models/laya`:

```sh
cargo fmt --all --check
cargo test --features metal --tests --locked
cargo clippy --features metal --all-targets --locked -- -D warnings
OPENJEV_MODEL_DIR=models/laya cargo test --release --features metal \
  --test test_openjev_local -- --ignored --nocapture --test-threads=1 \
  --skip real_preoptimization_executable
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=cpu \
  cargo bench --features metal --bench bench_openjev_local_inference -- laya --noplot
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench bench_openjev_local_inference -- laya --noplot
cargo bench --features metal --bench bench_openjev_decision -- \
  --noplot --sample-size 10 --measurement-time 1 --warm-up-time 1
```

Use `--features local` for CPU-only machines; the Metal parity test is compiled only with the Metal feature. `--offline` was used once dependencies were cached. Real GPU access and loopback binding were explicitly allowed for their respective checks.

## Not verified or implemented

- CUDA compilation, GPU execution, numerical parity, and performance: no NVIDIA hardware/toolkit was available on this Mac.
- A representative Jev quality/latency evaluation across distinct inputs: the paired comparison below repeats one public fixture ten times and does not establish general accuracy or service-level latency.
- Full upstream PyTorch parity, multilingual checkpoint behavior, and typed-decisions checkpoint behavior.
- Accuracy/calibration on a representative labeled user workload, adversarial robustness, long-running soak behavior, and service concurrency.
- Shared-prefix caching, quantization, training, a local HTTP server, and the optional Laya act/escalate head. Short-input Metal batching is now implemented and measured below.

These boundaries are deliberate statements of what the evidence establishes. Shape validation, passing tests, and fast inference do not make every model decision correct.

## Evo integration and official API verification

The application now loads `[TYPESAFE_TOKEN]` from `config/.secrets/secret_env.toml` through the actual Evo `UEnv` API. Secret loading runs before Tokio worker threads. The file was left unchanged, excluded from Git, and never copied into the build workspace. The wrapper suppresses upstream secret-parser logging, uses generic errors, removes the token entry from the global Evo map after reading, and zeroizes its owned token string on drop. This is not a guarantee that every temporary copy inside dependencies is zeroized.

On 2026-09-21, one real request to `https://api.typesafe.ai/v1/systemone`, requesting `jev-1.13.0`, successfully authenticated and returned validated Choice, Score, and Noul answers. The observed duration was **800 ms**, including network time, with **398 input tokens**. No outputs were collected for training or distillation. This one observation is not an inference benchmark or a quality comparison with Laya.

```sh
cargo test --locked --test test_openjev_live -- --ignored --nocapture
```

This explicitly opted-in test uses the default Evo secret path, or the file path in `OPENJEV_CONFIG`, and can incur API charges. Ordinary `cargo test` does not contact TypeSafe. Never pass the token itself as a command-line argument.

After the source reorganization, the real CPU/reference/Metal regression was rerun successfully with the same maximum errors shown above. Release executables and all examples built successfully; Clippy passed with warnings denied. The earlier Criterion measurements were not rerun for this configuration change.

Cargo currently emits a `duplicate key` diagnostic from the unrelated `app/app_agent_peer/Cargo.toml` inside the cached `evo_package_peer` dependency checkout. The selected application, test, example, and Clippy targets nevertheless finish successfully with exit status zero. That upstream manifest was not edited as part of this integration.

## Interactive backend switching, mouse, and timing

The 2026-09-21 TUI update passed **32 ordinary tests with Metal enabled** and **28 in the API-only configuration**, plus the opt-in resident-model switching test separately on real CPU and Metal. Clippy with warnings denied and formatting checks passed. All checks remain in `tests/`; the existing Criterion benchmarks remain in `benches/`.

New coverage verifies mouse hit regions across terminal sizes, help-modal isolation, wrapped/Unicode/scrolled cursor placement, wheel scrolling, action buttons, request cancellation on backend switch, preserved input/model/result provenance, no silent backend fallback, failed background loading and retry, and delayed-response timing. The real-model test switches Remote → Local → Remote → Local with a mock remote service and verifies the same resident model allocation is reused throughout.

A release TUI session was exercised at 120×36 using SGR mouse events. Clicking Evaluate in Remote made one real official Jev request (`jev-1.13.0`): **772 ms total**, **772 ms TTFB**. Clicking Local and then Evaluate reused the background-loaded Metal model and completed in **60 ms**. Switching back restored the remote model selection while the previous result remained labeled Local. Clicking Quit exited with status zero and disabled mouse capture and the alternate screen. These are individual functional observations, not comparative benchmarks.

The TUI explicitly reports **TTFT: N/A (no token stream)**. The official API returns complete structured JSON; the local model is a classifier. Neither exposes a generated-token event. Remote TTFB is measured at the first nonempty successful HTTP body chunk, including elapsed retry delays; total response time includes response parsing and validation. Demo/local TTFB is N/A. Exports carry `backend`, `ttft_ms`, `first_byte_ms`, and `elapsed_ms` without credentials. Model loading time is shown as a separate loading state and is excluded from local evaluation latency.

```sh
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo test --release --features metal --locked \
  --test test_openjev_interaction -- --ignored --nocapture
```

Repeat with `OPENJEV_DEVICE=cpu` for CPU residency/routing verification. CUDA still requires separate NVIDIA hardware verification. Existing neural-kernel parity results and inference benchmarks remain the prior measurements; this update changes orchestration, input handling, and instrumentation.

## GUI-style editing and layout update

The subsequent editable-studio update passed **38 ordinary tests with Metal** and **34 in the API-only configuration**, with Clippy warnings denied and formatting checks passing. Six new editing tests cover every state/question/criteria editor across all primitive types, multiline and Unicode paste, deletion, undo/redo, toolbar actions, drag/Shift-click/double-click selection, shared clipboard behavior, expanded panels, editable settings, atomic validation, and fixed timing indicators at four terminal sizes. A 90,000-character fixture checks that Select all does not truncate at the textarea library's u16 jump boundary.

Copy/Cut/Paste connect to the macOS clipboard in the executable. Automated editor tests deliberately use an in-memory clipboard so they do not overwrite the user's clipboard. Ctrl+C now copies; Ctrl+Q quits. Session Settings changes model, local checkpoint/device, and export directory without rewriting the private Evo configuration. Results remain read-only request snapshots and can be copied as JSON.

A separate opt-in Rust test, `test_openjev_visual`, exports synthetic Ratatui TestBackend cells for wide, compact, and Settings layouts. Their rendered images were inspected for layout, control visibility, selection/cursor feedback, and metric placement. The native Terminal application could not be inspected through the computer-use tool because that tool denies access to Terminal. The visual checks therefore establish the application's rendered buffer, not the exact font rendering of a particular terminal emulator.

```sh
OPENJEV_VISUAL_DIR=/tmp/openjev-visual cargo test --features metal --locked \
  --test test_openjev_visual -- --ignored
```

No new paid API call was necessary for this editor/layout change; the prior remote/local routing and real checkpoint checks remain recorded above. Earlier TUI microbenchmark values describe the older layout and should not be treated as measurements of this redesigned view.

The redesigned idle 120×36 TestBackend view was measured with Criterion (10 samples, 1 s warmup, 1 s measurement): **89.275 µs per render**, interval **88.941–89.532 µs**. Criterion reports approximately **8.7% more render time** than the older, simpler stored baseline. This measures in-memory widget rendering, excludes terminal I/O, and is not a claim that model inference changed.

## Adaptive local inference optimization

The 2026-09-21 optimization keeps the checkpoint, tokenizer, prompts, precision, temperatures, output schema, and context limits unchanged. It removes an unnecessary Q/K/V tensor copy in each of the 28 encoder layers and two decision-head layers. On Metal, a bounded scheduler groups up to four independent questions, sorts by input length, masks right padding in both the encoder and decision head, and reads probabilities back once per batch. Batch rows cannot attend to one another. The public response retains the original question IDs and counts only real input tokens.

Batching is restricted to complete inputs of at most 128 tokens and a longest/shortest length ratio of at most 1.5. CPU, CUDA, and longer Metal inputs remain sequential. An exploratory longer-input batch was slower (about 369 ms versus 345 ms sequential), so that path is not enabled. `evaluate_sequential` provides an explicit unbatched comparison path. No answer cache, quantization, distillation, model training, or new dependency was added.

Expanded tests exposed an existing Metal failure on longer sliding-window inputs: negative-infinity masks could produce NaNs in tiled attention. Masks now use a large finite negative bias, representable even when local and padding masks are combined in FP16. The tested normalized checkpoint activations give zero masked probability through floating-point underflow. Real-checkpoint comparisons now also cover a state containing 430 repeated words, within the existing 512-token complete-input limit.

### Fresh before/after measurements

Same Apple M5, same pinned weights, release build, one benchmark process at a time. Each Criterion case uses ten samples, two seconds of warmup, and at least ten seconds of measurement. Model loading is excluded; tokenization, inference, output validation, and probability readback are included. The original sources were explicitly rebuilt for the baseline. The saved original executable has SHA-256 `d14d58ee1b59564412a8832d7e0f9dee65956a1957a75ab00962278b81705d65`.

| Device and workload | Original mean | Optimized mean | Less elapsed time |
| --- | --- | --- | --- |
| Metal FP16, one Noul question | 26.707 ms | 25.452 ms | 4.70% |
| Metal FP16, three primitive questions | 81.450 ms | 71.119 ms | 12.68% |
| Metal FP16, twelve questions | 322.872 ms | 254.685 ms | 21.12% |
| CPU FP32, three primitive questions | 528.934 ms | 515.993 ms | 2.45% |

The three-question Metal mean's 95% confidence interval is 80.395–82.617 ms before and 71.083–71.164 ms after. CPU intervals are 526.964–530.773 ms before and 513.632–518.049 ms after. These intervals describe sampled means, not individual-request latency guarantees. The single-question fixture keeps `refund` from `triage.json`; the twelve-question fixture repeats its three questions under distinct IDs. All cases perform real inference on every iteration.

The longer three-question fixture now completes at a mean of 341.832 ms using the automatic sequential fallback; explicitly sequential inference measured 344.723 ms. Both execute the same numerical path, so their small timing difference is run variation, not a batching benefit. The old Metal path failed on this fixture and has no valid latency comparison. Cold-start performance, other Apple GPU generations, and CUDA speed remain unmeasured.

Complete raw estimates and per-sample timing/iteration counts are in [local-optimization.json](measurements/local-optimization.json). The benchmark sources remain exclusively in `benches/`; every test remains in `tests/`.

### Regression evidence

- 39 ordinary tests passed with Metal, and 34 passed with default API-only features. This includes request/response contracts, HTTP behavior, credential redaction, editable inputs, mouse interaction, shortcuts, cancellation, and response timing.
- Nine short requests / 27 decisions compared against the saved original executable separately on CPU and Metal had zero observed numeric differences, including probabilities, scores, and confidence. Discrete decisions, response metadata, and usage also matched. The numeric assertion requires differences below 0.00001; the observation does not establish bitwise equality for every possible input.
- Twenty-five mixed requests / 145 decisions compared automatic scheduling against sequential evaluation: zero observed numeric differences, checked separately on Metal and CPU. CPU retains sequential execution. Coverage includes 1, 3, 4, 5, 9, and 12 questions, multiple states, different option counts and temperature buckets, long-input fallback, and overflow rejection.
- A synthetic encoder test compares masked batches against independent Candle reference runs, changes padding IDs, and checks local/global attention and rotary positions. The real CPU/reference/Metal comparison now includes five states / fifteen decisions. Maximum probability differences remain 0.00000308 for native CPU and 0.00165898 for Metal FP16; Choice labels agree. This shares the Rust decision head with the reference and is not independent PyTorch parity.
- The real resident-model test passed separately on CPU and Metal while switching Remote → Local → Remote → Local, with a mock remote service. Clippy passed with warnings denied, formatting passed, and release binaries built successfully. No new paid official API call was made for this optimization.

These checks establish compatibility and numerical regression bounds on the tested inputs. Local Laya remains a different model from hosted Jev; API-shaped requests and responses do not establish identical predictions or quality. The previously recorded 800 ms official API observation was a single integration check; the later paired comparison is recorded below.

```sh
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal OPENJEV_BENCH_COMPARE=1 \
  cargo bench --features metal --bench bench_openjev_local_inference -- laya --noplot
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo test --release --features metal --test test_openjev_local \
  real_optimized -- --ignored --nocapture
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
OPENJEV_BASELINE_BINARY=/absolute/path/to/saved-original-openjev \
  cargo test --release --features metal --test test_openjev_local \
  real_preoptimization -- --ignored --nocapture
```

## Official Jev versus local: identical-input paired comparison

On 2026-09-21, `bench_openjev_remote_comparison` evaluated the exact same serialized Request content on the official `https://api.typesafe.ai/v1/systemone` API and the installed local Metal implementation: the public double-charge/refund state and all three questions from `examples/triage.json`, with `model` pinned to `jev-1.13.0` for both calls. Local evaluation retains that request field for compatibility but explicitly reports the actual `local-laya/laya` model.

The Rust benchmark made one official warmup evaluation and ten measured API evaluations, alternating local-first and remote-first order across ten serial pairs. It reused the HTTP client and resident local model, warmed local inference three times, included network/parsing/validation in remote time and tokenization/GPU readback/validation in local time, and did not cache responses. Evo loaded the token privately; the secret file was neither copied nor printed. No training or distillation was performed.

| Complete three-question request | Local Metal FP16 | Official Jev 1.13.0 |
| --- | --- | --- |
| Measured evaluations | 10 | 10 |
| Mean | 79.760 ms | 350.828 ms |
| Median | 83.768 ms | 340.687 ms |
| Observed minimum–maximum | 72.311–84.700 ms | 307.878–450.721 ms |

The ratio of sample means is **4.399×**, or **77.3% less elapsed time** locally on this input. The first official call was **806.350 ms** and is reported separately, outside the ten-call mean. Local model loading took **251.182 ms**, and the three excluded local warmups took **113.825, 71.835, and 71.682 ms**. This explains which startup costs are excluded; it does not isolate the causes of first-call latency. The paired protocol differs from uninterrupted local Criterion runs, so the earlier 71.119 ms local benchmark should not replace the paired 79.760 ms mean in this comparison.

Responses were stable within each backend across the ten measured calls, but **zero of ten response answer objects were numerically identical across backends**:

| Answer field | Local Laya | Official Jev |
| --- | --- | --- |
| Department Choice | billing | billing |
| P(billing) | 0.977872621 | 1.0 |
| Department confidence | 0.889490007 | 1.0 |
| Refund Noul value | 0.863875317 | 0.99 |
| Urgency Score, scale 0–2 | 1.917007015 | 2.0 |
| P(urgency = Today) | 0.931649027 | 1.0 |
| Urgency confidence | 0.740709104 | 1.0 |
| Reported input/output tokens | 150 / 0 | 398 / 69 |

Both select billing, both place the most urgency probability on Today, and both Noul values are above 0.5 (a comparison threshold, not an additional API Boolean field). Thus the practical conclusion agrees for this one fixture, while probabilities, confidence, score, model identity, and token usage differ. Matching decisions on repeated copies of one input do not establish general agreement or accuracy. A larger confidence value is not evidence of better calibration or correctness. Different token counts reflect backend accounting/preprocessing and do not mean different user request content was sent.

The full request, all ten paired responses, per-call timings, summary statistics, and excluded startup observations are preserved in [jev-vs-local.json](measurements/jev-vs-local.json). Ten observations of one fixture do not justify p95/p99 claims, load/concurrency claims, or a general speed/quality guarantee. Neither interface exposes a generated-token stream, so TTFT remains unavailable; remote first-byte times are recorded separately.

Reproduce with explicit live opt-in; this incurs official API usage:

```sh
OPENJEV_COMPARE_LIVE=1 OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench bench_openjev_remote_comparison
```

The benchmark is skipped without `OPENJEV_COMPARE_LIVE=1`, including ordinary `cargo bench` runs. Its disabled path and enabled ten-pair run both completed successfully; Clippy with warnings denied and formatting checks passed. The application inference and API client code were not changed for this comparison.
