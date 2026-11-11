# Legacy fragment compatibility

These excerpts retain routes, IDs, and heading labels from Eunoia's
`web/legacy-routes.json` and Qualpal's `docs/site-contract.json`.

The tests construct maintained pages and API targets locally. Eunoia's cases
exercise Julia titles, fields, methods, and signature IDs containing spaces.
Qualpal's cases exercise Doxygen article IDs and method IDs, including their
`r_` companions. The tests require no neighboring source checkout.
