# Wrapped Markdown

## Release notes

- `qualpal()` gains `refinement_starts` to control experimental continuous
  refinement for HSL and LCHab regions with the CIEDE2000 metric. The default
  uses five starts, which can change palettes and increase runtime. Set it to
  zero to disable refinement. Other metrics and fixed input sets skip refinement.

## Conduct

* Publishing others' private information, such as a physical or electronic
  address, without explicit permission
* Other conduct which could reasonably be considered inappropriate in a
  professional setting

## Algorithm

- a predefined set of RGB colors (or any other colors that can be converted to RGB,
  e.g., hex colors or HSL colors),
- sampled from (part of) the HSL or LCHab color spaces, or
- one of the predefined color palettes in the library, such as
  `"ColorBrewer:Set2"`

## Nesting

- Outer *emphasized
  continuation*, **strong
  continuation**, ~~struck
  continuation~~, and [linked
  continuation](https://example.test).
  - Inner text
    continues with `literal  spaces`.

    > Quoted text
    > continues.
    >
    > > Deeper quote
    > > continues.

  1. Ordered text
     continues.

- Next item.
