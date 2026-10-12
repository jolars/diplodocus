# R dataset fixtures

The `fruits`, `organisms`, `pain`, and `plants` Rd files are unchanged copies
from [eulerr commit 8030deb](https://github.com/jolars/eulerr/tree/8030deb7e6945af178b5717701389da12e036d5a/man).
They are distributed under eulerr's MIT license, included in `LICENSE`.
Tests copy them to temporary workspaces and exercise extraction, portable
snapshots, rendering, navigation, and search.

The synthetic `data-structures.Rd` topic supplies local targets for the four
unqualified links to R data structures. Resolving installed R help topics is a
separate compatibility task. The `pain` fixture's custom Disclaimer section
still produces the existing unsupported-markup warning and retains its raw
source in the snapshot.
