# Changelog

## [0.11.0](https://github.com/jolars/diplodocus/compare/v0.10.0...v0.11.0) (2026-10-28)

### Features

- support portable browser components ([`d3260c1`](https://github.com/jolars/diplodocus/commit/d3260c1e942f04bf6f6d7f1e4bd7f3c42a299886))
- add configurable nested sidebars ([`f677d82`](https://github.com/jolars/diplodocus/commit/f677d820a4e613161c87fd4e8b75b071fe17931c))

## [0.10.0](https://github.com/jolars/diplodocus/compare/v0.9.0...v0.10.0) (2026-10-27)

### Features

- support single-file content collections ([`c7623d6`](https://github.com/jolars/diplodocus/commit/c7623d619d2e8ccde43f85b8cf83a9972f627108))
- add light and dark theme selection ([`1cfe52f`](https://github.com/jolars/diplodocus/commit/1cfe52fc9a200bc32d259b2beb97ecd0a6cd3718))
- support full-width page layouts ([`a46b2d3`](https://github.com/jolars/diplodocus/commit/a46b2d35d1d8bae1fbac16917a7c22951ce420f7))

## [0.9.0](https://github.com/jolars/diplodocus/compare/v0.8.0...v0.9.0) (2026-10-27)

### Features

- render math in GFM documentation ([`1bc0793`](https://github.com/jolars/diplodocus/commit/1bc07934e586ecdf46fe48e137400e033ba9ae03))

### Bug Fixes

- **security:** override miniflare's undici dep ([`adbf07a`](https://github.com/jolars/diplodocus/commit/adbf07a6b6d39d3938f2ffd6558c661dbbefc507))

## [0.8.0](https://github.com/jolars/diplodocus/compare/v0.7.0...v0.8.0) (2026-10-26)

### Features

- **web:** add package summaries ([`5fff829`](https://github.com/jolars/diplodocus/commit/5fff8294fa301c668ad3722fda5d1f60f6b29ae7))

### Bug Fixes

- defer history writes until scrolling pauses ([`71f6bcf`](https://github.com/jolars/diplodocus/commit/71f6bcf842b8e52b5a1620c76d444c92617a5b49))

## [0.7.0](https://github.com/jolars/diplodocus/compare/v0.6.0...v0.7.0) (2026-10-23)

### Features

- generate responsive image variants ([`fdda840`](https://github.com/jolars/diplodocus/commit/fdda84086422b3df1173f5a50ecdae264d4bde2a))
- add intrinsic image dimensions ([`10acd4c`](https://github.com/jolars/diplodocus/commit/10acd4c2bc3d9ec2db16015dfc9878a33cdb91b9))
- allow declaring navigation for collections ([`e80a574`](https://github.com/jolars/diplodocus/commit/e80a5740fa667e4cedf98e1969bfb9480c37ea88))
- **web:** add icons for features ([`a001aae`](https://github.com/jolars/diplodocus/commit/a001aae86941037e53277721312605de51b7578e))

### Bug Fixes

- **ci:** include home in resolved content assertion ([`7d5eb67`](https://github.com/jolars/diplodocus/commit/7d5eb67e345c9f42cbcf0cb16acbf0e9a53670a3))
- **ci:** allow icons in home action assertion ([`119d521`](https://github.com/jolars/diplodocus/commit/119d521a07928631e74d0c8f122234c1e03f8634))
- **web:** fix transparent part of top navbar ([`025aaaa`](https://github.com/jolars/diplodocus/commit/025aaaa5a88785f898067f73c08bb62b1bf3c73a))
- **ui:** fix misalignment in "back to top" ([`d669397`](https://github.com/jolars/diplodocus/commit/d66939794cb7de3dbdb4e4c6e70c586f98ee113b))

### Performance Improvements

- **tests:** reduce repeated runtime execution ([`3e26693`](https://github.com/jolars/diplodocus/commit/3e26693eebcc6074581f6214cd5e3655685e62e9))

## [0.6.0](https://github.com/jolars/diplodocus/compare/v0.5.0...v0.6.0) (2026-10-23)

### Breaking changes

- **config:** replace TOML configuration with YAML ([`00f9bbd`](https://github.com/jolars/diplodocus/commit/00f9bbd89987e2715e8c617585292252723f1bde))

### Features

- **config:** replace TOML configuration with YAML ([`00f9bbd`](https://github.com/jolars/diplodocus/commit/00f9bbd89987e2715e8c617585292252723f1bde))
- **docs:** add a separate Reference section ([`e7838f1`](https://github.com/jolars/diplodocus/commit/e7838f1d5b8ab9597c2547e6b25f93905742f5af))
- **web:** redesign menu for mobile phones ([`7cd7277`](https://github.com/jolars/diplodocus/commit/7cd727712b0b0b545df4bfc358e172b812bf0414))
- **web:** improve navigation for mobile phones ([`55aba71`](https://github.com/jolars/diplodocus/commit/55aba71d966ea9ebc105966de2f50cb4e1852328))
- **ui:** redesign navbar and landing page ([`4b1f5b0`](https://github.com/jolars/diplodocus/commit/4b1f5b0b96677f5d1efed923d96a276ff9672b71))

### Bug Fixes

- **ci:** update docs test and patch Vite ([`dd9f5db`](https://github.com/jolars/diplodocus/commit/dd9f5db7154e61f3d37da3b5ebcf12b47afa6220))

## [0.5.0](https://github.com/jolars/diplodocus/compare/v0.4.0...v0.5.0) (2026-10-20)

### Features

- **ui:** add landing pages with a shared navbar ([`d9a6337`](https://github.com/jolars/diplodocus/commit/d9a633757c0ae72c3c13f3a289d4d8aa5e13d46d))
- **web:** add persistent client-side navigation ([`4f6efd8`](https://github.com/jolars/diplodocus/commit/4f6efd87895e0f586e90589cd7d3c6c0d8cb0734))
- **web:** separate code output from source ([`ea95010`](https://github.com/jolars/diplodocus/commit/ea95010e913908ac40bbf12be13c08aa61fccfc3))
- **web:** add tabsets ([`97243ee`](https://github.com/jolars/diplodocus/commit/97243ee9d68741635e2dea7f79c13748246c003f))
- add Rust documentation extraction ([`38ebc2a`](https://github.com/jolars/diplodocus/commit/38ebc2a1dc4b39944f21d228c517f08de9e1b19e))

### Bug Fixes

- format long signatures across parameter lines ([`954cc4c`](https://github.com/jolars/diplodocus/commit/954cc4cd18bbf522278f057e5640e8e81ffe8dce))
- remove copy buttons from API signatures ([`1dfb576`](https://github.com/jolars/diplodocus/commit/1dfb576734a0b870fc370936334fa471f0bcc018))
- stop flickering title ([`71d87ca`](https://github.com/jolars/diplodocus/commit/71d87ca43162e2c9edb65246c56ecd9ab239433c))
- validate every Julia checkout during extraction ([`737d195`](https://github.com/jolars/diplodocus/commit/737d195c897a0f0dab2649f4663c88621dc85a03))
- validate Python checkouts without doctests ([`2f434bc`](https://github.com/jolars/diplodocus/commit/2f434bcbea189a5e436ae1929102d686cc2c5ee6))
- restore documentation and VitePress CI checks ([`78bb2cb`](https://github.com/jolars/diplodocus/commit/78bb2cbffaffbaec5748d0ae6a9b0e70e9c1e5a6))
- improve site accessibility and preview reloads ([`7c4bca9`](https://github.com/jolars/diplodocus/commit/7c4bca9c8cec96de9c10e0833f9cc8e26e678476))
- **ui:** fix sidebar spacing ([`cf41c05`](https://github.com/jolars/diplodocus/commit/cf41c05d423d704daf126d2c84e4fda146163785))

## [0.4.0](https://github.com/jolars/diplodocus/compare/v0.3.0...v0.4.0) (2026-10-16)

### Features

- add watched extraction and VitePress example ([`ca4d8cf`](https://github.com/jolars/diplodocus/commit/ca4d8cf0c53ca86edb59cf9a96c1c147712aaa0e))
- support Markdown content negotiation ([`20ca2b0`](https://github.com/jolars/diplodocus/commit/20ca2b06340ec87f722fb5e7159a889b85d47dea))
- support Open Graph preview images ([`5b58724`](https://github.com/jolars/diplodocus/commit/5b58724d0ff8e133a3d8c33bdbe5f018bb5b08b1))
- host installers on diplodocus.cc ([`5a5b1cb`](https://github.com/jolars/diplodocus/commit/5a5b1cbf633b79de8d413dd7b0a5d46212f3a96a))
- switch to definition lists ([`2bd6311`](https://github.com/jolars/diplodocus/commit/2bd6311a18ae7e46a1bb6f86da8a396197f894ab))

### Bug Fixes

- align integration tests with current output ([`7b3fad7`](https://github.com/jolars/diplodocus/commit/7b3fad7f0a4c5c41df26edae3c5417ce90cac1ce))
- combine Julia function and method documentation ([`919f785`](https://github.com/jolars/diplodocus/commit/919f7853596fcb23ebd65429d508ba32d9a8355b))
- increase monospace font size ([`eb31b63`](https://github.com/jolars/diplodocus/commit/eb31b63fb4c6df9fcf2235906a128125e532da1e))
- **ui:** reduce margin in code snippets ([`8132d1e`](https://github.com/jolars/diplodocus/commit/8132d1e993c5a9c49b5622ff7bf1130b7e77b588))
- share description lists across API extractors ([`f2fcd58`](https://github.com/jolars/diplodocus/commit/f2fcd58a21c82a379a4381dae23890eb1d746849))

### Performance Improvements

- reduce test execution overhead ([`0a838f4`](https://github.com/jolars/diplodocus/commit/0a838f49a66dcb7161d3f2962aeeb56d1a227e83))

## [0.3.0](https://github.com/jolars/diplodocus/compare/v0.2.0...v0.3.0) (2026-10-15)

### Features

- add a flake for building diplodocus ([`487e1c0`](https://github.com/jolars/diplodocus/commit/487e1c0a661e6857fceb4a6624df10e2349acb2a))
- add binaries to releases ([`41eef5b`](https://github.com/jolars/diplodocus/commit/41eef5b3e3fcd06f23abbbdf6ef40f319bcba2c5))
- **python:** support embedded API references ([`72c8f35`](https://github.com/jolars/diplodocus/commit/72c8f353f4a597119213f200721f9bd250a1c7f6))

## [0.2.0](https://github.com/jolars/diplodocus/compare/v0.1.0...v0.2.0) (2026-10-15)

### Breaking changes

- use kebab-case configuration keys ([`360b0d6`](https://github.com/jolars/diplodocus/commit/360b0d68f02b1572d66c1c915b100656e12234fc))
- **snapshots:** retain presentation defaults ([`007866b`](https://github.com/jolars/diplodocus/commit/007866b793424d4baef55ad86fb1358df565b9e4))

### Features

- add snapshot fragments and Eunoia integration ([`fb3709f`](https://github.com/jolars/diplodocus/commit/fb3709fd8092f6914658981373299f910b201916))
- execute Python and Julia API doctests ([`8c79eea`](https://github.com/jolars/diplodocus/commit/8c79eeae24abf2fceea15886cc66c22a0d4d1d6d))
- **r:** improve reference presentation ([`d1c5862`](https://github.com/jolars/diplodocus/commit/d1c586226bbacc520ed85b43863c8170df969845))
- add offline datamonger example ([`1d0f387`](https://github.com/jolars/diplodocus/commit/1d0f387b7380e4c7e5373a4f4bb83e25adaa8211))
- **r:** support constants and deprecated help topics ([`514eca2`](https://github.com/jolars/diplodocus/commit/514eca2269ed10d22085ef1dbd141b86d6249d84))
- support R Markdown vignettes ([`dd35393`](https://github.com/jolars/diplodocus/commit/dd35393d258f8192b66c66564fd0ff10f1e048e7))
- execute Rd examples during extraction ([`0f2c836`](https://github.com/jolars/diplodocus/commit/0f2c83618cbcce7aa5f46879cee952cf23235387))
- **r:** present complete shared help topics ([`5288d97`](https://github.com/jolars/diplodocus/commit/5288d971ac7f4c27958ba4ca9b380648e1d6b0fa))
- add build-time syntax highlighting ([`609aeba`](https://github.com/jolars/diplodocus/commit/609aeba4c4f9d77d1d3b440143ce6f510666913f))
- **ui:** add logo ([`719ea36`](https://github.com/jolars/diplodocus/commit/719ea364956bd5114c6211472a44359dcdebb9e8))
- **ui:** add a footer ([`4c50004`](https://github.com/jolars/diplodocus/commit/4c50004ac0007dc8777c85d9151a72ec7d2124dd))
- support structured R documentation ([`2727efe`](https://github.com/jolars/diplodocus/commit/2727efefd74a6700750e198fb72c1b57ba0cd82a))
- add R package landing pages ([`7697fe7`](https://github.com/jolars/diplodocus/commit/7697fe7579329b3a5833070afe9b1b2eeb0e759c))
- support R dataset documentation topics ([`63b291f`](https://github.com/jolars/diplodocus/commit/63b291ff3795619968491b36472fe3c65708fad2))
- update contract to require language and dependencies ([`6fdd42e`](https://github.com/jolars/diplodocus/commit/6fdd42ec9e59627500f9693eabba3ac0613dfe2e))
- resolve R generics from installed metadata ([`793112c`](https://github.com/jolars/diplodocus/commit/793112c873ba21bddfb05a6d77cdf2a7228b7fdc))
- add static Julia extractor ([`a6eeb3e`](https://github.com/jolars/diplodocus/commit/a6eeb3ec8ecf9627bbcf43479191a43197b89645))
- use kebab-case configuration keys ([`360b0d6`](https://github.com/jolars/diplodocus/commit/360b0d68f02b1572d66c1c915b100656e12234fc))
- generate sitemaps and crawler metadata ([`9b801a5`](https://github.com/jolars/diplodocus/commit/9b801a5505e3f91a19972720a61fd8364554e6b5))
- **ui:** redesign left sidebar, add code styling ([`724085b`](https://github.com/jolars/diplodocus/commit/724085b4547690750ae453d069f750d0f3bc71f8))
- improve UI ([`451fd73`](https://github.com/jolars/diplodocus/commit/451fd7376cce97789779a606885d5867530ac4f4))
- **dev:** add Playwright browser workflow ([`8e1f154`](https://github.com/jolars/diplodocus/commit/8e1f154f842c463fd264fa8fe806417c60ad48b5))
- **snapshots:** retain presentation defaults ([`007866b`](https://github.com/jolars/diplodocus/commit/007866b793424d4baef55ad86fb1358df565b9e4))
- **execution:** cache complete page results ([`7949c4c`](https://github.com/jolars/diplodocus/commit/7949c4c32e2ac8917c4d08b9382495147ec4841b))
- **commands:** add snapshot builds and watched serving ([`7909c10`](https://github.com/jolars/diplodocus/commit/7909c10175dd1982edacd674d483e897c0981769))
- **validation:** connect static workspace checks ([`a2565c4`](https://github.com/jolars/diplodocus/commit/a2565c41352ac58c062e70de97c5b8a4a16fbb5f))
- **assembly:** compose workspace execution attempts ([`a5591ea`](https://github.com/jolars/diplodocus/commit/a5591ea0680e89fc23c331b40eb24a3e05c3bcbf))
- **execution:** compose supervised page engine ([`4917619`](https://github.com/jolars/diplodocus/commit/491761948adaae84c1c1e741180c7638e01bee20))
- **execution:** retain validated output evidence ([`b86c68e`](https://github.com/jolars/diplodocus/commit/b86c68eefb34a3f4785a33015f0832172f7c76c8))
- **execution:** bind validated page records ([`36d93e0`](https://github.com/jolars/diplodocus/commit/36d93e0e19e5647d3749fc35888bb1d527c24e7f))
- **execution:** add identity and input snapshots ([`15d9c77`](https://github.com/jolars/diplodocus/commit/15d9c770737066a13294b3b831d5012a277b0a75))
- **execution:** validate generated HTML and Markdown ([`a39a7c2`](https://github.com/jolars/diplodocus/commit/a39a7c2c7d729666c9a998f2f54ff939404a3ef9))
- **execution:** stage content-addressed figures ([`b31b44c`](https://github.com/jolars/diplodocus/commit/b31b44cbf2f39fcc2d11c5ac6574f86023a9df0e))
- **execution:** parse inert output fragments ([`cfff657`](https://github.com/jolars/diplodocus/commit/cfff657d95be66bcd55dbc57a082f6793a8be8cd))
- **execution:** collect typed cell outputs ([`661f26c`](https://github.com/jolars/diplodocus/commit/661f26cc1b56e97b7e2dd9cc8af1944cac9acec8))
- **execution:** freeze output and cache interfaces ([`f22d73f`](https://github.com/jolars/diplodocus/commit/f22d73f68ac047526948b555a05d9ea1822bfbf0))
- prepare evaluation, echo, output, etc ([`5c56119`](https://github.com/jolars/diplodocus/commit/5c56119ecbf74180731a6d0a0e4c2aeb32e07ea7))
- validate and prepare QMD execution options ([`aa0b8ca`](https://github.com/jolars/diplodocus/commit/aa0b8ca155b7a219ad2c4ff1f5881eaa9a45a0d3))
- execute cells in page-scoped kernel sessions ([`30d99d3`](https://github.com/jolars/diplodocus/commit/30d99d3e608bfbe5f159616b138c9f2cf3ea17b7))
- add Jupyter kernel discovery and startup ([`823b021`](https://github.com/jolars/diplodocus/commit/823b0218426212b94d50f956c7cca3827c032b51))
- define authored execution contract ([`387b548`](https://github.com/jolars/diplodocus/commit/387b54856d5aad609ece7cab2eeeab53d1a7b806))
- implement static R extraction ([`3558b29`](https://github.com/jolars/diplodocus/commit/3558b298393cc690d7db608447867cd1c8a8d70b))
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

- resolve repository links in package documents ([`03ade23`](https://github.com/jolars/diplodocus/commit/03ade2344b4d76033c73eaaf901ba5d11ef66398))
- **r:** resolve package page overrides before parsing ([`03a7d11`](https://github.com/jolars/diplodocus/commit/03a7d11c60507748dcb15ee612de37e6a81faa58))
- recognize annotated Python type aliases ([`60c0732`](https://github.com/jolars/diplodocus/commit/60c0732880817ee46b8273091f151d07ee7d8c6b))
- add runtime extraction for julia ([`a360f35`](https://github.com/jolars/diplodocus/commit/a360f35a18a7ef79c546ef922b29a5cc72305a07))
- **python:** display public reexport names ([`8dce5cc`](https://github.com/jolars/diplodocus/commit/8dce5ccf0ab79d2c3e07964245d420745a0bd229))
- **python:** consolidate overload reference pages ([`2d60207`](https://github.com/jolars/diplodocus/commit/2d602074b9ad726bf77b38e74c63cd630939b47d))
- bound unsupported Rd rendering ([`6be5b2c`](https://github.com/jolars/diplodocus/commit/6be5b2cd7e6a0e6c75f03b953e6feac0234fd90f))
- avoid ephemeral kernel ports and refresh goldens ([`2e5ad62`](https://github.com/jolars/diplodocus/commit/2e5ad62c26a2f96ed173862034d87ab938615af1))
- **ui:** remove extra margin in large size ([`91c26c9`](https://github.com/jolars/diplodocus/commit/91c26c9f6617b018d1678e6f10d335452405c838))
- resolve R package documentation and base generics ([`c46a782`](https://github.com/jolars/diplodocus/commit/c46a7826c67a7ca681fe1544711a769c1e396c46))
- accept static R useDynLib declarations ([`3e45e32`](https://github.com/jolars/diplodocus/commit/3e45e328f4a1921e93a56d0b47840e2b43f12aab))
- parse Python bytes literals in static signatures ([`749d0f6`](https://github.com/jolars/diplodocus/commit/749d0f68f6607cc5d595c4003ed4c73de098918f))
- extract Python context managers statically ([`55b40cb`](https://github.com/jolars/diplodocus/commit/55b40cbb161576a5d234e70c586e69abc42425f4))
- **deps:** override vulnerable sharp ([`9aee591`](https://github.com/jolars/diplodocus/commit/9aee591577b3a5a65715e7cacaa0fbfbc1c7f565))
- **ci:** skip unregistered workers.dev route ([`f93eb3f`](https://github.com/jolars/diplodocus/commit/f93eb3f12f01a5d950baca96c10bbd78f27bcf29))
- **extract:** exclude generated storage from inputs ([`a5d00bd`](https://github.com/jolars/diplodocus/commit/a5d00bd20168d1d61e04cf42c1652930d1585949))
- **extract:** resolve generated Markdown links ([`2e533be`](https://github.com/jolars/diplodocus/commit/2e533be036dc92beb8bc3e7d78dcd5445a08b168))
- **snapshots:** guard atomic publication ([`3ec4dad`](https://github.com/jolars/diplodocus/commit/3ec4dada9b61472871bcecfb19a829d34f91de50))
- **snapshots:** enforce canonical record encoding ([`f785565`](https://github.com/jolars/diplodocus/commit/f785565da250213c174675ce6fb12c0ee9cd3367))
- **snapshots:** validate nested records and references ([`e57df27`](https://github.com/jolars/diplodocus/commit/e57df27f2b1dcba2e9ee2b2eaf215f939123a4d0))
- **tests:** stabilize release CI checks ([`334aa22`](https://github.com/jolars/diplodocus/commit/334aa22ecc71d61e3f3a3229cb6e4f8b3584d194)), refs [#1](https://github.com/jolars/diplodocus/issues/1)
- **execution:** bind startup to launch identity ([`b0f8bdd`](https://github.com/jolars/diplodocus/commit/b0f8bdd50028ea96a210d9f0c901ab67d4a39847))
- **execution:** enforce monotonic phase deadlines ([`b0dad2a`](https://github.com/jolars/diplodocus/commit/b0dad2a1b93fbf79d11ffe3ab2275508c68a89a0))
- **execution:** retain kernel port leases ([`0bad798`](https://github.com/jolars/diplodocus/commit/0bad798bbfd843925184e3ea3ead9a4bf729d402))
- preserve provenance across version updates ([`de81b6e`](https://github.com/jolars/diplodocus/commit/de81b6e1f7a308b8c1b66f76a0b2ce670dff369c)), refs [#1](https://github.com/jolars/diplodocus/issues/1) and [#5](https://github.com/jolars/diplodocus/issues/5)
- keep Git provenance observation passive ([`d228997`](https://github.com/jolars/diplodocus/commit/d228997fa1b1a34ce8b895c590e9a805f9fbcca1))
- preserve filesystem path component order ([`03c7747`](https://github.com/jolars/diplodocus/commit/03c774769960613917e73cb7d458fa81e2bb28af))
- reject YAML merges in execution metadata ([`cafd3d9`](https://github.com/jolars/diplodocus/commit/cafd3d9288007522f7a4e4c8f2e9e5f596678ad1))
- stabilize kernel startup and version tests ([`27ae246`](https://github.com/jolars/diplodocus/commit/27ae246b96dfc109f6e9cfc6d2c34d2baccaffd8))
- use published arity-parser ([`f316408`](https://github.com/jolars/diplodocus/commit/f31640846d624add67d1843d35d7c5d54f4fa14c))

All notable changes to Diplodocus will be recorded in this file by Versionary.
