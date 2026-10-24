# Tiny Stats for Rust

Evaluate predictions with six small functions over slices of floating-point
values. Use `Score` when you also need to retain the number of observations.

## Installation

Add the example crate as a local dependency in your Cargo manifest, adjusting
the path for your project:

```toml
[dependencies]
tinystats = { path = "../diplodocus/examples/monorepo/rust" }
```

## Compare predictions

```rust
let observed = [1.0, 2.0, 3.0, 4.0];
let predicted = [1.0, 2.0, 3.0, 6.0];
let error = tinystats::mean_squared_error(&observed, &predicted);
assert_eq!(error, 1.0);
```

Read the [shared metric guide](../../docs/guides/choosing-a-metric.md) to choose
a score, or explore the API reference in the sidebar.
