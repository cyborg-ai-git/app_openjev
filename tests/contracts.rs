use openjev::{
    Answer, Evaluation, Request,
    app::{App, demo_response, parse_options},
};
use serde_json::json;

fn sample() -> Request {
    serde_json::from_str(include_str!("../examples/triage.json")).unwrap()
}

#[test]
fn all_three_primitives_roundtrip() {
    let request = sample();
    request.validate().unwrap();
    let response = demo_response(&request);
    let parsed: Evaluation =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    parsed.validate_for(&request).unwrap();
}

#[test]
fn rejects_invalid_states_empty_prompts_and_duplicate_options() {
    let mut request = sample();
    for bad in [
        json!(null),
        json!(true),
        json!(2),
        json!("  "),
        json!([]),
        json!({}),
    ] {
        request.state = bad;
        assert!(request.validate().is_err());
    }
    assert!(parse_options("a = first\na = duplicate").is_err());
    assert!(parse_options(" = missing key").is_err());
    let mut app = App::new("jev-latest".into(), "hello", true, None, ".".into());
    app.forms[0].instruction = openjev::app::editor("");
    assert!(app.request().is_err());
}

#[test]
fn rejects_response_with_missing_ids_wrong_types_and_bad_numbers() {
    let request = sample();
    let mut response = demo_response(&request);
    response.answers.remove("refund");
    assert!(response.validate_for(&request).is_err());
    let mut response = demo_response(&request);
    response
        .answers
        .insert("department".into(), Answer::Noul { noul: 0.8 });
    assert!(response.validate_for(&request).is_err());
    for bad in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut response = demo_response(&request);
        response
            .answers
            .insert("refund".into(), Answer::Noul { noul: bad });
        assert!(response.validate_for(&request).is_err());
    }
}

#[test]
fn rejects_foreign_choice_and_invalid_distribution() {
    let request = sample();
    let mut response = demo_response(&request);
    if let Answer::Choice { choice, .. } = response.answers.get_mut("department").unwrap() {
        *choice = "invented".into();
    }
    assert!(response.validate_for(&request).is_err());
    let mut response = demo_response(&request);
    if let Answer::Choice { probabilities, .. } = response.answers.get_mut("department").unwrap() {
        probabilities.insert("billing".into(), 0.0);
    }
    assert!(response.validate_for(&request).is_err());
}
