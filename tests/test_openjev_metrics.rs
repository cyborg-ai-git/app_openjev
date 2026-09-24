use openjev::metrics::{Observation, evaluate};

#[test]
fn perfect_predictions_have_zero_error_including_probability_one() {
    let data = [
        Observation {
            probabilities: vec![1.0, 0.0],
            label: 0,
        },
        Observation {
            probabilities: vec![0.0, 1.0],
            label: 1,
        },
    ];
    let result = evaluate(&data, 10).unwrap();
    assert_eq!(result.accuracy, 1.0);
    assert_eq!(result.brier, 0.0);
    assert_eq!(result.nll, 0.0);
    assert_eq!(result.ece, 0.0);
}

#[test]
fn uniform_balanced_predictions_are_calibrated_but_not_accurate() {
    let data = [
        Observation {
            probabilities: vec![0.5, 0.5],
            label: 0,
        },
        Observation {
            probabilities: vec![0.5, 0.5],
            label: 1,
        },
    ];
    let result = evaluate(&data, 10).unwrap();
    assert_eq!(result.accuracy, 0.5);
    assert_eq!(result.brier, 0.5);
    assert!((result.nll - 2.0_f64.ln()).abs() < 1e-12);
    assert_eq!(result.ece, 0.0);
}

#[test]
fn confident_errors_are_penalized_and_invalid_inputs_rejected() {
    let result = evaluate(
        &[Observation {
            probabilities: vec![0.99, 0.01],
            label: 1,
        }],
        15,
    )
    .unwrap();
    assert!(result.ece > 0.98 && result.brier > 1.9 && result.nll > 4.6);
    assert!(evaluate(&[], 15).is_err());
    assert!(
        evaluate(
            &[Observation {
                probabilities: vec![f64::NAN, 0.0],
                label: 0
            }],
            15
        )
        .is_err()
    );
}
