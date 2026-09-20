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
        "Four inputs / twelve decisions: CPU max probability error {max_cpu_error:.8}; Metal f16 {max_gpu_error:.8}"
    );
    assert!(max_cpu_error < 1e-4);
    assert!(max_gpu_error < 0.02);
}
