# Changelog

## [0.2.0](https://github.com/jolars/diplodocus/compare/v0.1.0...v0.2.0) (2026-09-18)

### Features
- complete static Python extraction ([`b279916`](https://github.com/jolars/diplodocus/commit/b27991621ff04079de5cbba1c2544c81b57438f0))
- reconcile Python public API identities ([`8648af2`](https://github.com/jolars/diplodocus/commit/8648af2971f41c831c487ab21a342abf92bd682b))
- translate Python docstrings into document IR ([`4026255`](https://github.com/jolars/diplodocus/commit/4026255f56baa88dcedff72865a58da76702fe63))
- parse Python package sources statically ([`bd439e7`](https://github.com/jolars/diplodocus/commit/bd439e756d4756b401ac87ed1567f8e12a827e66))
- define Python extraction diagnostics ([`ed38055`](https://github.com/jolars/diplodocus/commit/ed380557730e502a8ca7a52978cdb5ec7904157a))
- parse inert generated Markdown fragments ([`5680850`](https://github.com/jolars/diplodocus/commit/56808501f3c724c3b624eda7fcf02dcd46b0dc95))
- **ir:** add typed items and semantic identities ([`44521e2`](https://github.com/jolars/diplodocus/commit/44521e29bb4d30830040d87223ba0e63d01d8988))
- collect portable source provenance ([`ef0297a`](https://github.com/jolars/diplodocus/commit/ef0297ad64b40168a05a1ba4090ccf2ad840bfd0))
- define versioned workspace IR ([`b37f529`](https://github.com/jolars/diplodocus/commit/b37f52957b51d3feb73c81a3becdc15a69123077))
- validate workspace identities and references ([`e026408`](https://github.com/jolars/diplodocus/commit/e02640848b207a1ad5e2124551f0552805c9644d))
- define deterministic shared diagnostics ([`a8314f2`](https://github.com/jolars/diplodocus/commit/a8314f2ca32a43761f238f5a757fe686c314a7d5))
- resolve declared workspace paths ([`7db7f30`](https://github.com/jolars/diplodocus/commit/7db7f3051f5dfe42e0a7a0d5f171ab43811fecc3))
- enforce document execution authority ([`66117ed`](https://github.com/jolars/diplodocus/commit/66117ed8e2133a51a0e2e5beffcb1281923fd152))
- validate collection execution settings ([`b1351fe`](https://github.com/jolars/diplodocus/commit/b1351fe9abadeed0fc8cf82fe91404895e8049cc))
- parse workspace configuration ([`36967e2`](https://github.com/jolars/diplodocus/commit/36967e245aec74f20f5cfbc73c48324651d24518))
- rename to diplodocus ([`e02a466`](https://github.com/jolars/diplodocus/commit/e02a4662e598472ceacdec18757383fe82e5dfeb))
- add authored document adapter ([`2f8a825`](https://github.com/jolars/diplodocus/commit/2f8a8254ed9e1d6c188ad49ce0992c08936791d9))

### Bug Fixes
- preserve provenance across version updates ([`de81b6e`](https://github.com/jolars/diplodocus/commit/de81b6e1f7a308b8c1b66f76a0b2ce670dff369c)), refs [#1](https://github.com/jolars/diplodocus/issues/1) and [#5](https://github.com/jolars/diplodocus/issues/5)
- keep Git provenance observation passive ([`d228997`](https://github.com/jolars/diplodocus/commit/d228997fa1b1a34ce8b895c590e9a805f9fbcca1))
- preserve filesystem path component order ([`03c7747`](https://github.com/jolars/diplodocus/commit/03c774769960613917e73cb7d458fa81e2bb28af))
- reject YAML merges in execution metadata ([`cafd3d9`](https://github.com/jolars/diplodocus/commit/cafd3d9288007522f7a4e4c8f2e9e5f596678ad1))
- stabilize kernel startup and version tests ([`27ae246`](https://github.com/jolars/diplodocus/commit/27ae246b96dfc109f6e9cfc6d2c34d2baccaffd8))
- use published arity-parser ([`f316408`](https://github.com/jolars/diplodocus/commit/f31640846d624add67d1843d35d7c5d54f4fa14c))

All notable changes to Diplodocus will be recorded in this file by Versionary.
