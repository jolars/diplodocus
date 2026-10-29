module Demo
export fit, score

"""
Fit α inputs.

- `original_values`: requested areas keyed by
  combination string with *care* and **precision**.
- See [`score`](@ref) and [Julia](https://julialang.org).
  - Keep `nested` values.
  After the nested list.

  A separate paragraph.

      fit(1)

  After the code.

1. Follow [the integer method](@ref score(::Int))
   across wrapped lines.
2. Keep **ordered** text.
"""
function fit end

"Score integer inputs."
score(x::Int) = x
end
