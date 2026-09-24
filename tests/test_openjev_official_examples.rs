#[path = "support/u_openjev_official_cases.rs"]
mod cases;

use openjev::{Answer, Evaluation, Question, Usage};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn official_fixtures_are_valid_traceable_and_labels_are_not_invented_sdk_outputs() {
    let data = cases::dataset().unwrap();
    assert_eq!(data.schema_version, 1);
    assert_eq!(data.cases.len(), 21);
    let mut ids = BTreeSet::new();
    let mut questions = 0;
    for case in &data.cases {
        assert!(ids.insert(&case.id));
        assert!(!case.notes.is_empty());
        assert!(!case.source_ids.is_empty());
        case.request.validate().unwrap();
        assert_eq!(case.request.model, "jev-1.13.0");
        questions += case.request.questions.len();
        for id in &case.source_ids {
            let source = &data.sources[id];
            let url = source["url"].as_str().unwrap();
            assert!(
                url.starts_with("https://docs.typesafe.ai/")
                    || url.starts_with("https://github.com/typesafe-ai/")
            );
            assert_eq!(source["snapshot_sha256"].as_str().unwrap().len(), 64);
            if source["kind"] == "official_sdk_repository" {
                let revision = source["revision"].as_str().unwrap();
                assert_eq!(revision.len(), 40);
                assert!(url.contains(revision));
                assert!(
                    case.expected.is_empty(),
                    "SDK schema assertions are not semantic labels"
                );
            }
        }
        for (id, expected) in &case.expected {
            assert!(matches!(
                expected.basis.as_str(),
                "published_example" | "independent_semantic_label"
            ));
            if let Some(value) = expected.published_value {
                assert!(value.is_finite());
            }
            match &case.request.questions[id] {
                Question::Choice { criteria, .. } => {
                    assert!(criteria.contains_key(expected.decision.as_str().unwrap()))
                }
                Question::Score { criteria, .. } => {
                    for level in expected.decision.as_array().unwrap() {
                        assert!(level.as_str().unwrap().parse::<usize>().unwrap() < criteria.len());
                    }
                }
                Question::Noul { .. } => assert!(matches!(
                    expected.decision.as_str(),
                    Some("yes" | "no" | "uncertain")
                )),
            }
        }
    }
    assert_eq!(questions, 43);
    assert_eq!(data.cases.iter().filter(|c| !c.local_supported).count(), 3);
}

#[test]
fn comparison_distinguishes_agreement_from_probability_identity_and_reference_accuracy() {
    let data = cases::dataset().unwrap();
    let case = data
        .cases
        .iter()
        .find(|c| c.id == "docs_noul_resolved")
        .unwrap();
    let response = |noul| Evaluation {
        model: "fixture".into(),
        answers: [("is_human_escalation".into(), Answer::Noul { noul })].into(),
        usage: Usage {
            input_tokens: 1,
            output_tokens: 0,
        },
    };
    let comparison = cases::compare(case, &response(0.1), &response(0.2)).unwrap();
    assert_eq!(comparison[0]["decision_agrees"], true);
    assert_eq!(comparison[0]["exact_answer_agrees"], false);
    assert!((comparison[0]["total_variation_distance"].as_f64().unwrap() - 0.1).abs() < 1e-12);
    assert_eq!(
        cases::reference_checks(case, &response(0.9)).unwrap()[0]["matches_reference_decision"],
        false
    );
    assert_eq!(
        cases::decision(&Answer::Noul { noul: 0.5 }),
        json!("uncertain")
    );
    assert!(cases::compare(case, &response(f64::NAN), &response(0.2)).is_err());
    let score = Answer::Score {
        score: 0.5,
        legend: [("0".into(), "low".into()), ("1".into(), "high".into())].into(),
        probabilities: [("0".into(), 0.5), ("1".into(), 0.5)].into(),
        confidence: 0.0,
    };
    assert_eq!(cases::decision(&score), json!(["0", "1"]));
}

#[cfg(feature = "local")]
#[test]
#[ignore = "Requires OPENJEV_MODEL_DIR; checks every supported fixture on the actual local model"]
fn real_local_official_examples_preserve_capability_boundaries() {
    use openjev::local::{LocalModel, device};
    let path = std::env::var("OPENJEV_MODEL_DIR").unwrap();
    let name = std::env::var("OPENJEV_DEVICE").unwrap_or_else(|_| "cpu".into());
    let model = LocalModel::load(std::path::Path::new(&path), device(&name).unwrap()).unwrap();
    for case in cases::dataset().unwrap().cases {
        let result = model.evaluate(&case.request);
        if case.local_supported {
            let response = result.unwrap_or_else(|e| panic!("{}: {e}", case.id));
            response.validate_for(&case.request).unwrap();
            let checks = cases::reference_checks(&case, &response).unwrap();
            let matching = checks
                .iter()
                .filter(|c| c["matches_reference_decision"] == true)
                .count();
            println!(
                "{}: valid; reference decisions matched {matching}/{}",
                case.id,
                checks.len()
            );
        } else {
            let error = result.expect_err("Structured examples must not be silently flattened");
            assert!(error.to_string().contains("plain-text"));
        }
    }
}
