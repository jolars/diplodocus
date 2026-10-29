"Fit a model and return a [`SlopeFit`](@ref)."
function slope end

"Fit integer inputs."
slope(x::Int) = SlopeFit(x)

"A fitted model."
struct SlopeFit
    "The fitted coefficient."
    coefficient::Float64
end

"Predict from a fitted model."
predict(x::SlopeFit) = x.coefficient

"Read fitted coefficients."
coef(x::SlopeFit) = x.coefficient

"Plot support documented outside the plotting source."
plot_path(x::Int) = x
