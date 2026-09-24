# Official example prediction comparison

Measured on 2026-09-21 with the pinned local Laya checkpoint on Apple M5 / Metal FP16 and the official Jev API pinned to `jev-1.13.0`. This is an evaluation-only comparison. No training, distillation, prompt tuning, checkpoint changes, or application inference changes were made.

## Result

The two backends agreed on **27 of 37 comparable unique decisions (72.97%)**. They disagreed on ten decisions. None of the 37 complete answer objects was numerically identical. The second pass produced the same discrete decisions as the first for both backends; local answer objects were identical across passes, while remote probabilities, scores, or confidence changed on 15 of 21 cases. Two repetitions do not establish long-run determinism.

| Primitive | Comparable unique decisions | Same decision | Exact answer object |
| --- | --- | --- | --- |
| Choice | 7 | 6 | 0 |
| Noul | 22 | 16 | 0 |
| Score | 8 | 5 | 0 |
| Total | 37 | 27 | 0 |

Decision agreement means equal Choice labels, equal sets of highest-probability Score levels, or the same side of 0.5 for Noul. A Noul exactly equal to 0.5 is explicitly uncertain. Tied Score modes are preserved. Agreement does not mean that continuous scores, probabilities, or confidence match. Different confidence formulas also prevent interpreting their difference as a calibration comparison.

## Coverage and sources

There are **21 frozen requests / 43 questions**, replayed twice, plus one excluded remote warmup: 43 official API evaluations. The local model supports 18 requests / 37 questions. Three requests covering six questions use structured instructions or criteria and are rejected locally; the original payloads are preserved, without flattening or dropping questions. Jev successfully evaluated all 21 requests in both passes. There were no unexpected backend failures.

Examples were selected before execution from these primary sources:

- [Official quick start](https://docs.typesafe.ai/introduction/quickstart): three primitive questions about a Stripe integration failure.
- [Choice documentation](https://docs.typesafe.ai/primitives/choice): exchanges and a structured return-status request.
- [Score documentation](https://docs.typesafe.ai/primitives/score): bug severity, outfit formality, and candidate relevance from the interactive example data.
- [Noul documentation](https://docs.typesafe.ai/primitives/noul): human escalation, repeat contact, and five recorded message variants.
- [Jev 1.13 limitations](https://docs.typesafe.ai/model-jaggedness/jev-1.13): indexed fruit questions, cross-primitive refund questions, and a refund/other-request pair. No complementarity identity is assumed between separate Noul questions.
- [Official JavaScript SDK](https://github.com/typesafe-ai/typesafe-sdk-js/tree/66880ccded6cb642dc1809620c2b108c33730214): README, demo, and live integration payloads.
- [Official Python SDK](https://github.com/typesafe-ai/typesafe-sdk-python/tree/2ce5c65f13646cab6e6f782328194c9d85f3300a): README and live integration payloads.

The SDK README example is identical in both repositories and is counted once. The public SDK repositories contain API examples, not Jev model weights. Community playgrounds were not used. Every fixture includes source URLs, snapshot SHA-256 values, retrieval date, and immutable Git revisions where available. Website content is not revision-pinned; the imported request data is frozen locally. Explicit adaptations (model selector, expanded loop questions, assigned IDs, or unspecified null descriptions) are recorded per case.

SDK integration assertions only check types, ranges, and distributions. They were not misrepresented as semantic labels. Published documentation responses are recorded/illustrative examples, not immutable golden outputs or general ground truth. Among the **18 published decision labels comparable on both backends**, local matches 12 and Jev matches 18. An additional published structured-input label is supported only remotely and also matches. The eight fruit labels were independently assigned before evaluation: local gets 5/8, Jev 8/8. These eight simple labels are the only independently labeled accuracy subset; the broader 72.97% figure is backend agreement, not accuracy.

## Concrete disagreements

All values below are from the first measured pass; all cases and both passes remain in the raw report.

| Example | Local result | Official Jev result |
| --- | --- | --- |
| Stripe integration failure: department | billing (0.474); technical 0.276 | technical (0.920) |
| Repeat contact | Noul 0.444: no at 0.5 | Noul 0.940: yes |
| Indirect human escalation: bot question | Noul 0.837: yes | Noul 0.370: no |
| Outfit formality | modal level 1, casual; score 1.633 | modal level 2, business casual; score 1.860 |
| Candidate relevance | modal level 2; score 2.192 | modal level 3; score 2.560 |
| Indexed fruit: apple | Noul 0.192 | Noul 0.990 |
| Indexed fruit: banana | Noul 0.377 | Noul 0.990 |
| Indexed fruit: orange | Noul 0.196 | Noul 0.990 |
| Something other than a refund | Noul 0.749 | Noul 0.440 |
| SDK demo urgency, scale 0–3 | modal level 3; score 2.279 | modal level 2; score 2.240 |

The fruit errors concern the exact index-based questions over a JSON list; they do not establish how local inference would answer direct standalone fruit questions. The small urgency score difference also illustrates why modal disagreement and numeric error are separate measurements. No failure was removed or retuned after seeing the answers.

## Timing

The local model stays resident; the HTTP client is reused. Three local warmups and one remote warmup use the first case and are excluded; other cases can include their own first-use effects. Calls are serial, with backend order alternating. Remote time includes network, parsing, validation, and any normal client retry delays; local time includes tokenization, inference, GPU readback, and validation. No response cache is used.

Across **36 successful matched pairs** (18 cases, two passes), local mean latency is **63.788 ms**, versus **341.991 ms** for Jev: a ratio of **5.361×**. Medians are **49.549 ms** and **317.151 ms**. Observed ranges are **23.323–207.196 ms** and **258.766–499.862 ms**. Unsupported local requests are excluded from both sides of this timing comparison, not counted as near-zero local inference times. This mixes cases with different question counts and lengths; it is descriptive of this fixture suite, not a general performance guarantee. Two samples per case do not justify p95/p99 claims.

| Case | Matching decisions, first pass | Local mean ms, two passes | Jev mean ms, two passes |
| --- | --- | --- | --- |
| `docs_quickstart` | 2/3 | 107.62 | 318.88 |
| `docs_choice_exchange` | 1/1 | 33.40 | 335.29 |
| `docs_choice_structured` | 0/0 | Unsupported | 332.10 |
| `docs_score_severity` | 1/1 | 51.96 | 331.97 |
| `docs_score_formality` | 0/1 | 45.96 | 377.53 |
| `docs_score_relevance` | 0/1 | 51.99 | 352.87 |
| `docs_noul_repeat_contact` | 1/2 | 55.98 | 419.89 |
| `docs_noul_resolved` | 1/1 | 31.41 | 316.81 |
| `docs_noul_password` | 1/1 | 31.35 | 327.86 |
| `docs_noul_urgent` | 1/1 | 30.80 | 343.71 |
| `docs_noul_bot` | 0/1 | 29.21 | 336.67 |
| `docs_noul_invoice` | 1/1 | 32.25 | 304.16 |
| `docs_fruit_indirection` | 5/8 | 203.93 | 403.26 |
| `docs_refund_cross_primitive` | 2/2 | 50.36 | 316.80 |
| `docs_refund_negation` | 1/2 | 53.43 | 282.34 |
| `sdk_readme_billing` | 1/1 | 29.17 | 370.96 |
| `sdk_js_demo` | 3/4 | 129.20 | 382.46 |
| `sdk_js_live` | 3/3 | 96.50 | 349.64 |
| `sdk_js_rich` | 0/0 | Unsupported | 341.96 |
| `sdk_python_rich` | 0/0 | Unsupported | 367.81 |
| `sdk_python_pydantic` | 3/3 | 83.68 | 284.73 |

`0/0` identifies a capability gap with no paired predictions. It is neither a correct prediction nor a semantic failure.

## Files and verification

- [Frozen fixtures and provenance](../tests/fixtures/official_examples.json)
- [Rust integration tests](../tests/test_openjev_official_examples.rs)
- [Rust comparison helpers](../tests/support/u_openjev_official_cases.rs)
- [Bounded live comparison benchmark](../benches/bench_openjev_official_examples.rs)
- [Complete measured responses and metrics](measurements/official-examples.json)

The new offline tests check fixture validity, source provenance, supported labels, uncertainty/tie handling, rejection of invalid probabilities, and the distinction between nominal agreement and numerical identity. The ignored real-model test checks all 18 supported cases and explicit rejection of the three unsupported structured cases; it reports reference matches without pretending that schema validity proves prediction correctness. It passed on Metal. The full Metal-enabled suite passed 41 ordinary tests, the API-only suite passed 36, Clippy passed with warnings denied, and formatting passed. No inference/UI code was changed, so existing application binaries do not need replacement.

Ordinary tests and benchmarks do not call the official API. The live comparison skips unless explicitly enabled, uses the existing private Evo credential loader, writes partial results after each pair, and stops on authorization failure. No token is included in any fixture or report. Original SDK MIT notices are retained in `docs/licenses/`.

```sh
cargo test --features metal --test test_openjev_official_examples --locked
OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo test --release --features metal --test test_openjev_official_examples \
  -- --ignored --nocapture
# Explicit opt-in: one warmup plus 42 measured official API evaluations.
OPENJEV_OFFICIAL_LIVE=1 OPENJEV_MODEL_DIR=models/laya OPENJEV_DEVICE=metal \
  cargo bench --features metal --bench bench_openjev_official_examples --locked
```

`OPENJEV_CONFIG` optionally selects the private Evo configuration file by path. `OPENJEV_OFFICIAL_OUTPUT` overrides the report destination. API charges may apply to explicit live runs. The fixture is a convenience sample of public examples, not a blinded or representative evaluation dataset; published cases may be unusually favorable to the service whose documentation contains them.
