//! Prediction metrics for observed and predicted values.
//!
//! Start with [`mean_squared_error`] and inspect [`residuals`] to understand
//! where predictions differ from observations.
//!
//! ```
//! let observed = [1.0, 2.0, 3.0];
//! let predicted = [1.0, 2.0, 4.0];
//! assert_eq!(tinystats::mean_squared_error(&observed, &predicted), 1.0 / 3.0);
//! ```

/// A metric value with its sample size.
#[derive(Clone, Debug)]
pub struct Score {
    /// Computed metric value.
    pub value: f64,
    /// Number of observations.
    pub observations: usize,
}

impl Score {
    /// Compute [`mean_squared_error`] and retain the sample size.
    ///
    /// ```
    /// let score = tinystats::Score::mse(&[1.0, 2.0], &[2.0, 2.0]);
    /// assert_eq!(score.value, 0.5);
    /// assert_eq!(score.observations, 2);
    /// ```
    pub fn mse(observed: &[f64], predicted: &[f64]) -> Self {
        Self {
            value: mean_squared_error(observed, predicted),
            observations: observed.len(),
        }
    }
}

/// Return observed values minus predicted values.
pub fn residuals(observed: &[f64], predicted: &[f64]) -> Vec<f64> {
    assert_eq!(observed.len(), predicted.len());
    assert!(!observed.is_empty());
    observed
        .iter()
        .zip(predicted)
        .map(|(actual, fitted)| actual - fitted)
        .collect()
}

/// Return the mean squared error of the predictions.
pub fn mean_squared_error(observed: &[f64], predicted: &[f64]) -> f64 {
    residuals(observed, predicted)
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        / observed.len() as f64
}

/// Return the average absolute residual.
pub fn mean_absolute_error(observed: &[f64], predicted: &[f64]) -> f64 {
    residuals(observed, predicted)
        .iter()
        .map(|value| value.abs())
        .sum::<f64>()
        / observed.len() as f64
}

/// Return the square root of [`mean_squared_error`].
pub fn root_mean_squared_error(observed: &[f64], predicted: &[f64]) -> f64 {
    mean_squared_error(observed, predicted).sqrt()
}

/// Return the average signed residual.
pub fn mean_error(observed: &[f64], predicted: &[f64]) -> f64 {
    residuals(observed, predicted).iter().sum::<f64>() / observed.len() as f64
}

/// Return the fraction of observed variation explained by the predictions.
pub fn r_squared(observed: &[f64], predicted: &[f64]) -> f64 {
    let errors = residuals(observed, predicted);
    let mean = observed.iter().sum::<f64>() / observed.len() as f64;
    let total = observed
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>();
    1.0 - errors.iter().map(|value| value * value).sum::<f64>() / total
}
