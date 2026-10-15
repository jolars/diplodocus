# Offline article fixture

`mixed.csv` is an exact copy of the CC0-1.0 mixed CSV artifact from Datamonger's
shared conformance corpus at commit
`0071087bca9fe0f379987a8653676b372d512544`. Its SHA-256 is
`8a46d390c070778b3617d11ba9a0ea0cb3516a2c54aeafdc53c9196367208c1e`.

The R Markdown regression uses this small fixture to check package data lookup,
executed tables and figures, and generation without the source package. It does
not require a Datamonger checkout or its runtime dependencies. Datamonger's own
offline article separately verifies its public retrieval and decoder APIs.
