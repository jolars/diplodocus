# Tiny Stats for Julia

Compare numeric predictions with observed values using the same six operations
as the Python, R, and Rust packages.

## Installation

From the Diplodocus repository root, add the example package to your Julia
environment:

```julia
using Pkg
Pkg.develop(path="examples/monorepo/julia")
```

## Compare predictions

```julia
using TinyStats

observed = [1, 2, 3, 4]
predicted = [1, 2, 3, 6]
mean_squared_error(observed, predicted)
# 1.0
```

Read the [shared metric guide](../docs/guides/choosing-a-metric.md) to choose a
score, or explore the API reference in the sidebar.
