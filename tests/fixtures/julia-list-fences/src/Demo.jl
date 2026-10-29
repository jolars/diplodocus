module Demo
export fit

"""
Fit α inputs with **wrapped
bold** and *wrapped
emphasis*, plus `wrapped
code`. See [fit
reference](@ref
fit).

- Areas:
  ```julia
  α = 1
    fit(α)
  ```
  After the fence.
  - Nested example:
    ~~~~julia
    fit(2)
    ~~~~
  After the nested list.

      fit(3)

  After the indented code.

1. Membership:
   ```julia
   fit(4)
   ```

!!! note
    ```julia
    β = 5
      fit(β)
    ```
"""
function fit end
end
