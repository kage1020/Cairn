use cairn_lang_core::voxel_diff::{CONVERGENCE, Counts, Metrics, floor3, percent};

#[test]
fn each_metric_is_a_ratio_of_the_cells_counted() {
    let counts = Counts {
        structure_blocks: 10,
        built_blocks: 8,
        both_blocks: 7,
        matched: 6,
        states_matched: 5,
        residual: 2,
    };
    assert_eq!(
        counts.metrics(),
        Metrics {
            block_iou: 6.0 / 11.0,
            state_accuracy: 5.0 / 6.0,
            residual_ratio: 2.0 / 8.0,
        }
    );
    assert_eq!(counts.differing(), 6);
}

#[test]
fn a_comparison_of_no_blocks_converges() {
    let metrics = Counts::default().metrics();
    assert_eq!(
        metrics,
        Metrics {
            block_iou: 1.0,
            state_accuracy: 1.0,
            residual_ratio: 0.0,
        }
    );
    assert!(metrics.converged(&CONVERGENCE));
}

#[test]
fn a_metric_at_its_threshold_converges() {
    let counts = Counts {
        structure_blocks: 200,
        built_blocks: 200,
        both_blocks: 200,
        matched: 197,
        states_matched: 197,
        residual: 10,
    };
    assert!(counts.metrics().converged(&CONVERGENCE));
}

#[test]
fn a_shortfall_names_each_metric_that_misses_its_threshold() {
    let metrics = Metrics {
        block_iou: 0.962_6,
        state_accuracy: 0.99,
        residual_ratio: 0.050_1,
    };
    assert_eq!(
        metrics.shortfalls(&CONVERGENCE),
        [
            "block IoU = 0.962 < threshold 0.985",
            "state_accuracy = 0.990 < threshold 0.995",
            "residual raw = 5.1% > threshold 5%",
        ]
    );
    assert!(!metrics.converged(&CONVERGENCE));
}

#[test]
fn a_printed_metric_rounds_toward_failing() {
    assert_eq!(
        [0.984_99, 0.985, 1.0, 0.0].map(floor3),
        ["0.984", "0.985", "1.000", "0.000"]
    );
    assert_eq!(
        [0.050_01, 0.05, 0.07, 0.035, 0.0, 1.0].map(percent),
        ["5.1%", "5%", "7%", "3.5%", "0%", "100%"]
    );
}
