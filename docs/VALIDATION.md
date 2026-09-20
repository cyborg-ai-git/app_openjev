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

The Metal-enabled integration suite has **20 ordinary tests**, all passing. Two additional real-checkpoint tests are ignored by default and were explicitly run successfully. All test implementations are in `tests/`; source files contain no test modules.

| Area | Checks |
| --- | --- |
| Contracts | Three primitive round trips; invalid/empty state; duplicate options; mismatched response IDs/types; nonfinite/out-of-range probabilities; invalid option distributions |
| HTTP | Actual loopback server; correct POST body and bearer header; no retry on unauthorized replies; bounded rate-limit retries; timeout and malformed JSON; HTTPS enforcement; oversized response rejection; no echo of malformed server data |
| TUI | All primitives at multiple terminal dimensions; Unicode; raw JSON and help; extreme scroll offsets; retained edits; cancellation; exports linked to the submitted snapshot; visible inputs at 70×24 |
| Local preprocessing | Stable softmax; Noul ordering; Score expectation; temperature buckets; marker sanitization; overflow rejection |
| Encoder | Deterministic synthetic weights with local/global attention, compared against Candle's ModernBERT reference at multiple sequence lengths |
| Metrics | Perfect predictions; calibrated uniform predictions; confidently wrong predictions; invalid data |
| Real checkpoint | All three primitives; independent-question agreement; native CPU and Metal probabilities compared with the reference encoder |

`cargo fmt --all --check` and `cargo clippy --features metal --all-targets --locked --offline -- -D warnings` pass. The default API/demo feature configuration was also tested successfully. Release CPU/Metal inference and the model downloader were exercised as executables. An interactive PTY smoke check opened the TUI, evaluated the demo with F5, exited with Ctrl-Q, and confirmed that the original terminal settings were restored. When GPU access is unavailable in the sandbox, Metal startup returns an explanatory error rather than panicking.

The installed local TUI was also exercised with real Metal weights. Two consecutive F5 evaluations of its default single Choice question completed in 52 ms and 44 ms, respectively, through the background worker. These are smoke observations, not Criterion measurements; the three-question benchmark below uses a different workload.

The real parity check uses four English inputs and three questions per input. The maximum absolute probability differences against the full decision pipeline using Candle's FP32 reference encoder were:

| Compared backend | Maximum absolute probability difference | Test tolerance |
| --- | --- | --- |
| Native CPU FP32 | 0.00000308 | less than 0.0001 |
| Native Metal FP16 | 0.00165898 | less than 0.02 |

This is a small numerical regression check. The reference and optimized pipelines share the Rust decision-head implementation, so agreement does not independently validate that head against PyTorch. It also does not measure classification accuracy, prove calibration, or guarantee the same error bound on every input.

## Warm inference

Workload: [examples/triage.json](../examples/triage.json), one shared short state and three questions, covering Choice, Noul, and Score. The current engine evaluates those questions sequentially. Criterion loads the model outside the timing loop, warms for two seconds, requests ten samples over at least ten seconds, and includes tokenization and probability readback inside the timed evaluation. CPU and Metal runs were performed separately.

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
  --test local -- --ignored --nocapture --test-threads=1
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=cpu \
  cargo bench --features metal --bench local_inference -- laya --noplot
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench local_inference -- laya --noplot
cargo bench --features metal --bench decision -- \
  --noplot --sample-size 10 --measurement-time 1 --warm-up-time 1
```

Use `--features local` for CPU-only machines; the Metal parity test is compiled only with the Metal feature. `--offline` was used once dependencies were cached. Real GPU access and loopback binding were explicitly allowed for their respective checks.

## Not verified or implemented

- CUDA compilation, GPU execution, numerical parity, and performance: no NVIDIA hardware/toolkit was available on this Mac.
- A real TypeSafe API call or direct Jev quality/latency comparison: no paid API request was made.
- Full upstream PyTorch parity, multilingual checkpoint behavior, and typed-decisions checkpoint behavior.
- Accuracy/calibration on a representative labeled user workload, adversarial robustness, long-running soak behavior, and service concurrency.
- Batching, shared-prefix caching, quantization, training, a local HTTP server, and the optional Laya act/escalate head.

These boundaries are deliberate statements of what the evidence establishes. Shape validation, passing tests, and fast inference do not make every model decision correct.
