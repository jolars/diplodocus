# API Reference

```@index
```

## Modules

SLOPE contains a single module:

```@docs
SLOPE.SLOPE
```

## Model Fitting

The main entry point is [`slope`](@ref), which returns a [`SlopeFit`](@ref).

```@docs
SLOPE.slope
SLOPE.SlopeFit
SLOPE.predict
SLOPE.coef
```

## Cross-Validation

```@docs
SLOPE.slopecv
SLOPE.best_α
SLOPE.best_model
SLOPE.refit
SLOPE.SlopeGridResult
SLOPE.SlopeCvResult
```

## Plotting

```@autodocs
Modules = [SLOPE]
Pages = ["plots.jl"]
Private = false
```

## Utilities

```@docs
SLOPE.regweights
SLOPE._normalize
```

The fitted coefficient remains [addressable](@ref `SLOPE.SlopeFit.coefficient`).
