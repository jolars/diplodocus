//! Prediction metrics for the VitePress watcher fixture.

/// Compute the mean squared difference between observations and predictions.
pub fn mean_squared_error(actual: &[f64], predicted: &[f64]) -> f64 {
    actual
        .iter()
        .zip(predicted)
        .map(|(actual, predicted)| (actual - predicted).powi(2))
        .sum::<f64>()
        / actual.len() as f64
}
