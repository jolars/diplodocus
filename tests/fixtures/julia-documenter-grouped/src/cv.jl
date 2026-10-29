"Cross-validate a model."
slopecv(x) = x

"Choose the best regularization parameter."
best_α(x) = x

"Choose the best model."
best_model(x) = x

"Refit the selected model."
refit(x) = x

"Results along a regularization grid."
struct SlopeGridResult
    models::Int
end

"Cross-validation results."
struct SlopeCvResult
    grid::SlopeGridResult
end
