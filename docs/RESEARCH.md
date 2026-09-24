# TypeSafe AI, Jev, and a local Rust implementation

Research date: 2026-09-21. Scope: all 109 pages linked by the official documentation index were retrieved and indexed; the model/API/concept pages, failure modes, public repositories, and relevant independent inference implementations were examined for reproducibility. This is a bounded review of public evidence, not a claim to have inspected private source or every page on the Internet. The index includes generated SDK reference pages as well as conceptual documentation. The retrieval manifest records every indexed URL and checksum.

## What model does TypeSafe use?

The published service currently identifies the model as **Jev 1.13**, addressed by `jev-1.13.0`. Both `jev-latest` and `jev-preview` resolve to it in the reviewed documentation. It accepts text and JSON representations of text, with a 64k total request budget and a 32k state-plus-longest-question budget. These are service specifications, not a disclosure of the neural backbone. [Official models](https://docs.typesafe.ai/models).

TypeSafe describes System One as direct, structured decision inference and describes Jev's post-training as reinforcement learning for calibrated decisions (RLCD). That does not identify BERT, ModernBERT, a decoder family, a parameter count, or a complete training recipe. No official Jev checkpoint or complete neural implementation was located in the reviewed public material. Consequently, an exact offline Jev reproduction cannot be claimed from these sources. [System One](https://docs.typesafe.ai/concepts/system-one), [AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer), [TypeSafe repositories](https://github.com/typesafe-ai).

**There is no verified basis here for saying “Jev is based on BERT.”** A black-box architecture investigation proposes decoder/attention and possible MoE hypotheses. Its behavioral experiments are interesting but do not reveal the private implementation; multiple architectures can exhibit similar behavior. [Archer Hume's original investigation](https://archerhume.com/posts/jevs-architecture-unmasked).

The likely source of the BERT association is **Laya**, an independent open model: its English checkpoint explicitly uses ModernBERT-large with a learned decision head. That fact must not be transferred to Jev. [Laya model card](https://huggingface.co/convaiinnovations/laya).

## Public behavior we can reproduce

The official SDK index lists Python and JavaScript/TypeScript. It also permits direct HTTP calls from any language; it does not list an official Rust SDK in the reviewed version. OpenJev supplies its own Rust client, separately from its local inference backend. [Official SDK index](https://docs.typesafe.ai/sdk).

`POST /v1/systemone` accepts shared `state`, a model identifier, and keyed questions. Choice yields a distribution and selected key; Score yields a distribution over ordered levels and an expectation; Noul yields the probability of yes. Choice permits up to 255 alternatives and Score uses 2–10 levels. Questions are evaluated against the state without being a conversational chain. This contract is sufficient to implement a Rust client and a compatible local application interface. It is not sufficient to reconstruct the learned mapping. [HTTP contract](https://docs.typesafe.ai/api), [Primitives](https://docs.typesafe.ai/primitives), [Parallel questions](https://docs.typesafe.ai/cookbooks/parallel_questions).

Jev's service can process questions in parallel while sharing the state. Our current local engine evaluates each question separately. This preserves isolation but repeats encoder work; neither Jev's server throughput nor shared-state efficiency is reproduced. The TUI submits one editable question at a time; the headless interface accepts multiple questions.

The official confidence page describes a statistic of the answer distribution. Its interactive Choice demo uses normalized maximum probability, `(K * max(p) - 1) / (K - 1)`, and explicitly calls its three-option calculation an approximation. This is not evidence for a universally specified production formula, particularly for Score. Laya's public implementation instead uses `1 - entropy(p)/ln(K)`. OpenJev retains Laya's formula locally and passes through the server's confidence remotely. These numbers are not interchangeable. [TypeSafe confidence documentation](https://docs.typesafe.ai/confidence), [Laya source](https://github.com/NandhaKishorM/laya).

Type-safe output is not a guarantee of factual correctness, consistent logic between independently phrased questions, or resistance to adversarial input. TypeSafe itself documents failures around arithmetic, date comparison, indirection, distracting context, and manipulated state. Our implementation validates shapes and numerical ranges; it cannot validate the semantic truth of arbitrary predictions. [Jev 1.13 failure modes](https://docs.typesafe.ai/model-jaggedness/jev-1.13).

## Open projects and what they contribute

| Project | Public evidence and role | Relevance to this crate |
| --- | --- | --- |
| [TypeSafe organization](https://github.com/typesafe-ai) | SDKs, examples, and related public projects | API behavior; no Jev weights located |
| [System One adapter](https://github.com/typesafe-ai/system-one-adapter-python) | Public adapter implementation | Contract reference; not a Rust Jev inference engine |
| [Laya](https://github.com/NandhaKishorM/laya) | Open checkpoints, decision-head and training code | Selected basis for native local inference |
| [Laya MLX](https://github.com/mizorewww/laya-mlx) | Independent Apple GPU port and validation methodology | Cross-check of head layout, activations, and precision considerations; not a dependency |
| [Laya Core ML](https://github.com/mizorewww/laya-coreml) | Apple-specific deployment alternative | Reviewed alternative, not bundled or used |
| [Kev](https://github.com/jaredpalmer/kev) | Qwen3-family typed-decision architecture | Shows that the interface is not inherently BERT-specific |
| [NanoJev](https://github.com/TianyuCodings/NanoJev) | Qwen3-0.6B-based experimental reproduction | Alternative research direction; no Jev equivalence established |
| [jevlike](https://github.com/vinnylarouge/jevlike) | Independent prototype | Additional public experiment, not an official release |
| [SemIf](https://github.com/TheoLeeCJ/SemIf) | Separate semantic decision project, formerly called OpenJev | Unrelated to this new Rust crate despite the historical name |
| [GLiClass](https://github.com/Knowledgator/GLiClass) | Open label-conditioned classification | Relevant alternative for classification; not the full Jev contract |
| [Candle](https://github.com/huggingface/candle) | Rust tensor operations and CPU/CUDA/Metal backends | Actual inference dependency |
| [mistral.rs](https://github.com/ericlbuehler/mistral.rs) | Rust inference framework | Alternative runtime ecosystem; not used by this crate |

These are references, not a claim that every project's accuracy, license, model variant, or benchmark was independently validated. The downloaded Laya revision and the sources ported into this crate are attributed in `NOTICE`.

## The implemented local architecture

The validated artifact is `convaiinnovations/laya`, revision `c5d78730f3493e4fe16d61507ef4b78eef7318cf`. Its published family includes an English ModernBERT checkpoint, a multilingual mmBERT variant, and a typed-decisions variant. Only the English checkpoint was downloaded and exercised here. Its model card reports approximately 421M parameters and a 512-token input limit. It also reports weaknesses outside English, overconfidence, and task-dependent results: these are reasons to measure on the actual target workload. [Checkpoint and model card](https://huggingface.co/convaiinnovations/laya).

The Rust forward path is:

1. Validate the typed request. Render structured state as compact JSON. Sort Choice keys deterministically.
2. Tokenize the question and options, insert one marker token per option, and append the state. Reject input that exceeds checkpoint budgets. Literal marker text from user content is removed before tokenization so it cannot create extra scoring positions.
3. Run ModernBERT with rotary position embeddings and alternating global/local attention.
4. Add the question-type embedding. Run the two pre-normalized decision transformer layers with their ReLU feed-forward activation.
5. Gather option-marker representations, apply the learned scorer, and divide logits by the checkpoint's type/option-count temperature.
6. Apply stable softmax and construct Choice, Score, or Noul directly. Validate the result before returning it.

The ModernBERT details follow its published architecture and Candle's reference implementation. The independent MLX implementation helped cross-check decision-head details. Our tests compare the custom encoder with Candle on synthetic tensors and compare full native CPU/Metal results on real weights. They do not establish complete PyTorch equivalence because no independent upstream golden tensor fixtures were imported or generated. [ModernBERT paper](https://arxiv.org/abs/2412.13663), [Candle](https://github.com/huggingface/candle), [MLX reference](https://github.com/mizorewww/laya-mlx).

The optional upstream act/escalate head is not exposed. No automatic checkpoint router, quantization, training loop, or shared-prefix cache is implemented. The Metal runtime batches up to four independent short questions with masked padding; CPU/CUDA and longer inputs remain sequential. Prompt ordering and strict overflow rejection differ from some upstream convenience behavior and can affect predictions. Raising a configuration limit alone does not establish model quality at a longer context.

## Rust, CPU, CUDA, and Metal

The application, tokenizer integration, model orchestration, typed heads, HTTP client, TUI, utilities, tests, and benchmarks are Rust. Candle handles tensor execution. CPU uses the native tensor path; CUDA maps supported operations to NVIDIA GPU facilities; Metal maps them to Apple's GPU facilities. Metal is a GPU backend, not a separate third kind of processor.

“All application code is Rust” does not mean that vendor GPU drivers or dependency kernels are literally Rust source. Candle's CUDA and Metal backends necessarily use their platform APIs and kernel toolchains. No Python interpreter, PyTorch process, MLX process, or external model server is needed to run this project. [Candle backend documentation](https://github.com/huggingface/candle).

This implementation caches rotary tables and local masks, keeps model weights resident in the TUI, and uses fused scaled dot-product attention on supported Metal shapes. FP32 is the CPU default; FP16 is the Metal default. GPU probability readback completes before a result is returned, so the measured latency includes completed work. CPU and Metal are tested; CUDA support remains an unverified build/runtime path on this Mac.

Our implementation assessment: more speed may come from batching, smaller validated checkpoints, quantization, and backend-specific kernels. Each changes either numerical behavior, workload scheduling, or model capacity and needs its own accuracy and latency measurements. Current measurements are recorded separately from third-party performance claims in `VALIDATION.md`.

## What is still needed to reproduce a complete decision system?

The delivered crate reproduces an operational typed-decision interface and runs an available open model locally. An exact Jev clone would additionally require its weights, tokenizer, architecture, training distribution, objectives, and serving optimizations, which were not found publicly. Rewriting an API client in Rust cannot supply those missing artifacts.

For an independently trained alternative, an engineering plan would require labeled or distribution-valued training examples across each primitive, explicit held-out splits, a trainable backbone/head, objective and optimizer implementation, task-specific calibration, robustness evaluation, and versioned deployment. Laya publishes an RLCD-style training approach, but this crate implements inference, not that training pipeline. [Laya training source](https://github.com/NandhaKishorM/laya).

A proper scoring objective encourages honest probability reporting under its assumptions; it does not prove empirical calibration under optimization error or distribution shift. Evaluate accuracy together with Brier score, negative log likelihood, ECE, and action-specific error costs. `src/utility/u_openjev_metrics.rs` and the Rust metrics example provide the first four measurements for user-supplied labels. Thresholds must be chosen using representative validation data; a confidence number alone cannot establish reliability.

## Reproducible evidence

- `typesafe-documentation-manifest.json`: all 109 indexed official URLs, content sizes, and SHA-256 hashes from the retrieval run.
- `validated-checkpoint.json`: repository revision and hashes for all five model/tokenizer/configuration artifacts used in the real-model checks.
- `VALIDATION.md`: hardware, commands, observed outcomes, and unverified scope.
- `../examples/research.rs`: Rust documentation downloader; stored content is reference data and is never executed.
- `../tests/`: executable contract, UI, numerical, and backend checks.
- `../benches/`: reproducible Criterion benchmarks.

The official site and repository heads can change. Refresh the documentation cache for a new review and pin both the model revision and Cargo lockfile when comparing results.
