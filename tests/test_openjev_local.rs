#![cfg(feature = "local")]
use openjev::{
    Answer, Question, Request,
    local::{LocalModel, answer, confidence, device, softmax, temp_bucket},
};
use serde_json::json;

#[test]
fn probability_math_is_stable_and_ordered() {
    let p = softmax(&[10000.0, 10001.0], 1.0).unwrap();
    assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    assert!(p[1] > p[0]);
    assert!(confidence(&[0.5, 0.5]).abs() < 1e-12);
    assert!((confidence(&[1.0, 0.0]) - 1.0).abs() < 1e-12);
    assert!(softmax(&[f64::NAN], 1.0).is_err());
    assert!(softmax(&[1.0], 0.0).is_err());
    assert_eq!(temp_bucket(0, 7), "choice:6-10");
    let q = Question::Noul {
        instructions: json!("True?"),
        criteria: None,
    };
    assert!(matches!(answer(&q, &[0.2, 0.8]).unwrap(), Answer::Noul { noul } if noul == 0.8));
    let q = Question::Score {
        instructions: json!("Rate"),
        criteria: vec![json!("low"), json!("mid"), json!("high")],
    };
    assert!(
        matches!(answer(&q, &[0.1, 0.2, 0.7]).unwrap(), Answer::Score { score, .. } if (score - 1.6).abs() < 1e-12)
    );
}

#[test]
#[ignore = "Requires local weights: OPENJEV_MODEL_DIR; optional OPENJEV_DEVICE"]
fn real_checkpoint_all_primitives_and_question_isolation() {
    let directory = std::env::var("OPENJEV_MODEL_DIR").expect("Missing OPENJEV_MODEL_DIR");
    let device_name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let model = LocalModel::load(
        std::path::Path::new(&directory),
        device(&device_name).unwrap(),
    )
    .unwrap();
    let request: Request = serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    let response = model.evaluate(&request).unwrap();
    response.validate_for(&request).unwrap();
    let mut single = request.clone();
    single.questions.retain(|id, _| id == "refund");
    let isolated = model.evaluate(&single).unwrap();
    let Answer::Noul { noul: full } = response.answers["refund"] else {
        panic!()
    };
    let Answer::Noul { noul: alone } = isolated.answers["refund"] else {
        panic!()
    };
    assert!((full - alone).abs() < 1e-5);
}

#[test]
fn input_builder_preserves_markers_and_rejects_overflow() {
    use tokenizers::{
        Tokenizer, models::wordlevel::WordLevel, pre_tokenizers::whitespace::Whitespace,
    };
    let vocabulary = [
        ("[UNK]".into(), 0),
        ("[CLS]".into(), 1),
        ("[SEP]".into(), 2),
        ("[MASK]".into(), 3),
    ]
    .into_iter()
    .collect();
    let mut tokenizer = Tokenizer::new(
        WordLevel::builder()
            .vocab(vocabulary)
            .unk_token("[UNK]".into())
            .build()
            .unwrap(),
    );
    tokenizer.with_pre_tokenizer(Some(Whitespace));
    let question = Question::Noul {
        instructions: json!("A [MASK] marker should not be injected"),
        criteria: None,
    };
    let prepared = openjev::local::prepare::prepare(
        &tokenizer,
        &json!("User [MASK] text"),
        &question,
        100,
        60,
        [1, 2, 3],
    )
    .unwrap();
    assert_eq!(prepared.kind, 2);
    assert_eq!(prepared.markers.len(), 2);
    assert_eq!(prepared.ids.iter().filter(|id| **id == 3).count(), 2);
    assert!(
        prepared
            .markers
            .iter()
            .all(|position| prepared.ids[*position as usize] == 3)
    );
    assert!(
        openjev::local::prepare::prepare(
            &tokenizer,
            &json!("word ".repeat(120)),
            &question,
            100,
            60,
            [1, 2, 3]
        )
        .is_err()
    );
}

#[cfg(feature = "metal")]
#[test]
#[ignore = "Requires checkpoint and real Apple GPU access"]
fn real_cpu_reference_native_and_metal_probabilities_agree() {
    use candle_core::{DType, Device};
    use openjev::local::LoadOptions;
    let path = std::env::var("OPENJEV_MODEL_DIR").expect("Missing OPENJEV_MODEL_DIR");
    let path = std::path::Path::new(&path);
    let mut request: Request =
        serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    let states = [
        "I was charged twice for the same order. Please refund the extra payment today.",
        "The integration crashes on startup. There are no billing issues and no refund is requested.",
        "I have a general question about your opening hours. No hurry.",
        "The package is damaged. Please help me understand my options for returning it.",
        // Cross several local-attention tiles; this previously produced Metal NaNs.
        &format!("{} Please refund the payment today.", "word ".repeat(430)),
    ];
    let reference = LocalModel::load_with_options(
        path,
        Device::Cpu,
        LoadOptions {
            dtype: DType::F32,
            reference_encoder: true,
        },
    )
    .unwrap();
    let expected: Vec<_> = states
        .iter()
        .map(|state| {
            request.state = json!(state);
            reference.evaluate(&request).unwrap()
        })
        .collect();
    drop(reference);
    let cpu = LocalModel::load(path, Device::Cpu).unwrap();
    let cpu_results: Vec<_> = states
        .iter()
        .map(|state| {
            request.state = json!(state);
            cpu.evaluate(&request).unwrap()
        })
        .collect();
    drop(cpu);
    let gpu = LocalModel::load(path, device("metal").unwrap()).unwrap();
    let mut max_cpu_error = 0.0_f64;
    let mut max_gpu_error = 0.0_f64;
    fn values(answer: &Answer) -> Vec<f64> {
        match answer {
            Answer::Noul { noul } => vec![*noul],
            Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
                probabilities.values().copied().collect()
            }
        }
    }
    for (index, state) in states.iter().enumerate() {
        request.state = json!(state);
        let result = gpu.evaluate(&request).unwrap();
        for id in request.questions.keys() {
            if let (
                Answer::Choice { choice: actual, .. },
                Answer::Choice {
                    choice: expected, ..
                },
            ) = (&result.answers[id], &expected[index].answers[id])
            {
                assert_eq!(
                    actual, expected,
                    "CPU reference and Metal chose different labels"
                );
            }
            let gold = values(&expected[index].answers[id]);
            for (a, b) in gold.iter().zip(values(&cpu_results[index].answers[id])) {
                max_cpu_error = max_cpu_error.max((a - b).abs());
            }
            for (a, b) in gold.iter().zip(values(&result.answers[id])) {
                max_gpu_error = max_gpu_error.max((a - b).abs());
            }
        }
    }
    println!(
        "Five inputs / fifteen decisions: CPU max probability error {max_cpu_error:.8}; Metal f16 {max_gpu_error:.8}"
    );
    assert!(max_cpu_error < 1e-4);
    assert!(max_gpu_error < 0.02);
}

#[test]
#[ignore = "Requires checkpoint; use OPENJEV_DEVICE=metal to exercise batching on a real GPU"]
fn real_optimized_and_sequential_requests_agree_across_lengths_and_batch_boundaries() {
    let path = std::env::var("OPENJEV_MODEL_DIR").expect("Missing OPENJEV_MODEL_DIR");
    let device_name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let model =
        LocalModel::load(std::path::Path::new(&path), device(&device_name).unwrap()).unwrap();
    let fixture: Request = serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    let mut max_error = 0.0_f64;
    let states = [
        "I was charged twice. Please refund the extra payment today.".to_owned(),
        "The integration crashes. No payment problem; I do not want a refund.".to_owned(),
        "What are your opening hours? This can wait.".to_owned(),
        format!(
            "{} I need a refund today.",
            "The customer has an active account and previous orders. ".repeat(24)
        ),
    ];
    for (case, state) in states.into_iter().enumerate() {
        for count in [1, 3, 4, 5, 9, 12] {
            let mut request = fixture.clone();
            request.state = json!(state);
            request.questions = fixture
                .questions
                .values()
                .cycle()
                .take(count)
                .enumerate()
                .map(|(i, q)| (format!("question_{i:02}"), q.clone()))
                .collect();
            let expected = model.evaluate_sequential(&request).unwrap();
            let actual = model.evaluate(&request).unwrap();
            actual.validate_for(&request).unwrap();
            assert_eq!(actual.model, expected.model);
            assert_eq!(actual.usage.input_tokens, expected.usage.input_tokens);
            assert_eq!(actual.usage.output_tokens, expected.usage.output_tokens);
            for id in request.questions.keys() {
                let a = serde_json::to_value(&actual.answers[id]).unwrap();
                let b = serde_json::to_value(&expected.answers[id]).unwrap();
                compare_values(
                    &a,
                    &b,
                    &mut max_error,
                    &format!("case {case}, count {count}, {id}"),
                );
            }
        }
    }
    // Exercise different marker counts and every temperature bucket in the same request.
    let mut varied = fixture.clone();
    for count in [2, 6, 11] {
        varied.questions.insert(
            format!("choice_{count}"),
            Question::Choice {
                instructions: json!("Choose the best description."),
                criteria: (0..count)
                    .map(|i| (format!("label_{i}"), json!(format!("Description {i}"))))
                    .collect(),
            },
        );
        let levels = count.min(10);
        varied.questions.insert(
            format!("score_{levels}"),
            Question::Score {
                instructions: json!("Rate urgency."),
                criteria: (0..levels).map(|i| json!(format!("Level {i}"))).collect(),
            },
        );
    }
    compare_values(
        &serde_json::to_value(model.evaluate(&varied).unwrap()).unwrap(),
        &serde_json::to_value(model.evaluate_sequential(&varied).unwrap()).unwrap(),
        &mut max_error,
        "mixed option counts",
    );
    // Errors remain errors; no truncation or cached result may hide changed input.
    let mut invalid = fixture.clone();
    invalid.state = json!("word ".repeat(600));
    assert!(model.evaluate(&invalid).is_err());
    assert!(model.evaluate_sequential(&invalid).is_err());
    println!("{device_name}: maximum numerical difference across 25 requests: {max_error:.8}");
}

#[test]
#[ignore = "Requires OPENJEV_BASELINE_BINARY pointing to the unmodified release executable"]
fn real_preoptimization_executable_and_optimized_model_agree() {
    let binary = std::env::var("OPENJEV_BASELINE_BINARY").expect("Missing OPENJEV_BASELINE_BINARY");
    let path = std::env::var("OPENJEV_MODEL_DIR").expect("Missing OPENJEV_MODEL_DIR");
    let device_name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let model =
        LocalModel::load(std::path::Path::new(&path), device(&device_name).unwrap()).unwrap();
    let fixture: Request = serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let request_path = directory.path().join("request.json");
    let mut max_error = 0.0;
    for state in [
        fixture.state.clone(),
        json!("The integration crashes. No payment problem; I do not want a refund."),
        json!("What are your opening hours? This can wait."),
    ] {
        for count in [1, 3, 5] {
            let mut request = fixture.clone();
            request.state = state.clone();
            request.questions = fixture
                .questions
                .values()
                .cycle()
                .take(count)
                .enumerate()
                .map(|(i, q)| (format!("question_{i:02}"), q.clone()))
                .collect();
            std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
            let output = std::process::Command::new(&binary)
                .arg("--local-model")
                .arg(&path)
                .arg("--device")
                .arg(&device_name)
                .arg("--request")
                .arg(&request_path)
                .output()
                .unwrap();
            assert!(output.status.success(), "baseline executable failed");
            let expected: openjev::Evaluation = serde_json::from_slice(&output.stdout).unwrap();
            let actual = model.evaluate(&request).unwrap();
            compare_values(
                &serde_json::to_value(actual).unwrap(),
                &serde_json::to_value(expected).unwrap(),
                &mut max_error,
                "preoptimization executable",
            );
        }
    }
    println!(
        "{device_name}: maximum difference against the original executable across 9 requests: {max_error:.8}"
    );
}

fn compare_values(
    a: &serde_json::Value,
    b: &serde_json::Value,
    max_error: &mut f64,
    context: &str,
) {
    match (a, b) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
            let difference = (a.as_f64().unwrap() - b.as_f64().unwrap()).abs();
            *max_error = max_error.max(difference);
            assert!(
                difference < 1e-5,
                "{context}: numerical difference {difference}"
            );
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (key, a) in a {
                compare_values(a, &b[key], max_error, context);
            }
        }
        _ => assert_eq!(a, b, "{context}: a decision or metadata changed"),
    }
}
