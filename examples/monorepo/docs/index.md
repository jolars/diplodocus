# Tiny Stats

Tiny Stats evaluates numeric predictions in Python, R, and Julia. The packages
share six operations, so you can learn the ideas once and use any of the three languages.

## Start with the guide

[Install Tiny Stats](getting-started/installation.md), then follow the
[quick start](getting-started/quick-start.md) to compare two sets of predictions.
The packages are deliberately small, working examples for Diplodocus.

## Learn how to evaluate predictions

- [Comparing predictions](guides/comparing-predictions.md) works through squared error.
- [Choosing a metric](guides/choosing-a-metric.md) compares error magnitude, bias, and R squared.
- [Inspecting residuals](guides/inspecting-residuals.md) explains what a single score can hide.

## Choose a language

Browse the **Reference** branches in the sidebar or open a function directly:

- **Python:** [`pystats::tinystats.mean_squared_error`]
- **R:** [`rstats::mean_squared_error`]
- **Julia:** [`juliastats::TinyStats.mean_squared_error`]

Each function links to its counterparts under **Same API in**. The
[Python workflow](../python/docs/evaluating-models.md) shows how to evaluate
several models with a small loop.
