# Changelog

## [0.7.0](https://github.com/contriboss/vein/compare/v0.6.0...v0.7.0) (2026-10-31)


### Features

* add crates exports extraction module foundation ([d9cdff4](https://github.com/contriboss/vein/commit/d9cdff48b22403ffedf4ed5b25c4dbd53aa2050b))
* add TreeSitter language parsers, consolidate imports ([5181a73](https://github.com/contriboss/vein/commit/5181a730bcf95c9b5cb405ddc2c652925a4bff8c))
* complete crates exports framework ready for production ([a709fde](https://github.com/contriboss/vein/commit/a709fde97ae39e5b9788f6548d9d1559cd1b9260))
* finalize queue injection for crate extraction pipeline ([7b84426](https://github.com/contriboss/vein/commit/7b84426e76aee3bff2be209c7539e7395eba75be))
* implement TreeSitter AST walker for Rust crate exports ([7a5004d](https://github.com/contriboss/vein/commit/7a5004dc4db29245d13dca16b1da033f27f9291c))
* wire crate extraction as stampeded AsyncJob ([d0edebf](https://github.com/contriboss/vein/commit/d0edebf0cac000b60e543a55121c3bb3cfee0d35))


### Bug Fixes

* **ci:** drop deprecated cross-platform-actions run input for FreeBSD ([c06a4a8](https://github.com/contriboss/vein/commit/c06a4a85534788f172a8c10269a09f6c777cf464))
* **ci:** stop recompiling for the Docker image, fix arm64 QEMU hang ([839efc7](https://github.com/contriboss/vein/commit/839efc7d4bcf3fa0f830f1ead895abbf5b7c8784))
* pin vein-adapter workspace dependency to 0.6.0 ([0027fbe](https://github.com/contriboss/vein/commit/0027fbeca3d8a67ccd8e2f387b441724326d672c))
* simplify crate tarball collection for exports extraction ([f26fdc6](https://github.com/contriboss/vein/commit/f26fdc6b3d74c3d2853cec1b65fc179b4581792f))

## [0.6.0](https://github.com/contriboss/vein/compare/v0.5.1...v0.6.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* maintenance sync from forgejo

### Features

* code context tools on the MCP, GitHits the first provider ([160fb17](https://github.com/contriboss/vein/commit/160fb175d7879d02d49170e03339f38dc17cab98))
* **config:** VEIN_DATABASE_URL overrides database.url ([aefb304](https://github.com/contriboss/vein/commit/aefb3045351d5b7d13767dcbbd0eecd9a8efbf22))
* **delay:** release delay policy, full-registry search, and promotion jobs ([74dbd8f](https://github.com/contriboss/vein/commit/74dbd8f78dca749740462f62b2d94556ab9f18ae))
* **docs:** package documentation at /docs and vein://.../readme|api ([d9ae700](https://github.com/contriboss/vein/commit/d9ae7008ab4b5b6d63596f54b3d1be2e70558183))
* each registry is a Cargo feature (gems, crates, npm) ([fc74ca0](https://github.com/contriboss/vein/commit/fc74ca04eed5f2a12a79fa78aa1bef61f76e926e))
* vein as a LAN package oracle, deployed to harbor with launchbay ([899f9ca](https://github.com/contriboss/vein/commit/899f9ca086fc0262a54bfa2d7771e03fbb4eefdd))


### Miscellaneous Chores

* maintenance sync from forgejo ([979313b](https://github.com/contriboss/vein/commit/979313b3e0dbb3e0500fa6c016c5997b2f22e23a))
* **release:** pin the next release to 0.6.0 ([656ec27](https://github.com/contriboss/vein/commit/656ec27dd9485f1a7fed4ac241efbc463251826d))

## [0.5.1](https://github.com/contriboss/vein/compare/v0.5.0...v0.5.1) (2026-07-30)


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * vein-adapter bumped from 0.4.0 to 0.5.0

## [0.5.0](https://github.com/contriboss/vein/compare/v0.4.0...v0.5.0) (2026-07-30)


### Features

* add npm support ([#11](https://github.com/contriboss/vein/issues/11)) ([9ac5967](https://github.com/contriboss/vein/commit/9ac5967088852e3d7d0a2c881165663d8ea791ed))
* added docker compose file ([#9](https://github.com/contriboss/vein/issues/9)) ([04773d6](https://github.com/contriboss/vein/commit/04773d601407296e067007a1683f2d11ed2223ff))
* **admin:** add Datastar foundation ([7f9910e](https://github.com/contriboss/vein/commit/7f9910efa075fd05386991997e5d6e910a66a682))
* **admin:** add Datastar refresh to dashboard stats ([e4940ae](https://github.com/contriboss/vein/commit/e4940aef9f373b84c4088c1e045d682e6f9a8b00))
* **admin:** add Datastar search to catalog ([de69c7d](https://github.com/contriboss/vein/commit/de69c7d6cbc151d8e0bafc4d075b2a0ed7ef072e))
* **admin:** migrate dashboard to Tera templates ([9650d85](https://github.com/contriboss/vein/commit/9650d857a919525dcbe709648e9fd21ee276efe4))
* drop loco-rs and use rama&datastar ([#8](https://github.com/contriboss/vein/issues/8)) ([6d74999](https://github.com/contriboss/vein/commit/6d74999eb39c7e586f7bf49715cf9b4fe4f8f387))
* **indexer:** add tree-sitter Ruby symbol indexing with per-version storage ([8e0ae2a](https://github.com/contriboss/vein/commit/8e0ae2a1f870c91d44f2f60fb521a2a1d48f2b63))
* introduce crates proxing ([#10](https://github.com/contriboss/vein/issues/10)) ([28668a5](https://github.com/contriboss/vein/commit/28668a5c3307ad48ad78dfa2f5304bea625cb5df))
* prepare vein-adapter, vein-admin, and vein-admin-migration for crates.io publishing ([495af02](https://github.com/contriboss/vein/commit/495af02a3e1c56b473414c2cde609fb67382caff))
* Treesitter and goblin ([c2ebe25](https://github.com/contriboss/vein/commit/c2ebe25ece5a28b6c2c6f0fccb2e820c1208914d))
* **validate:** add binary architecture validation with goblin for platform mismatch detection ([e39eef6](https://github.com/contriboss/vein/commit/e39eef6ac5f436c3f3042ccd614984703359b8aa))


### Bug Fixes

* add cargo-workspace plugin so release PR dep versions stay consistent ([8e6a24f](https://github.com/contriboss/vein/commit/8e6a24fcc88e73b338e5be16090e50668ccc10b1))
* address all 14 Copilot PR review issues (NULL comparisons, platform parsing, error handling, tests, code quality) ([d69b3e2](https://github.com/contriboss/vein/commit/d69b3e2c539105196fc6c0dfade5265d6dda99f2))
* address clippy warnings ([966671a](https://github.com/contriboss/vein/commit/966671a53dd68adbc21e7d54a2fb8f15e8a46f9b))
* address copilot review concerns ([2737931](https://github.com/contriboss/vein/commit/2737931ca52328508bf6802344fc7e800b1bb01c))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * vein-adapter bumped from 0.3.0 to 0.4.0

## [0.4.0](https://github.com/contriboss/vein/compare/v0.3.0...v0.4.0) (2025-12-28)


### Features

* **ci:** add force_build option for manual releases ([1349a64](https://github.com/contriboss/vein/commit/1349a643acf4b3ef4229cda5c668d216832c5954))


### Bug Fixes

* **ci:** add cmake dep and disable fail-fast ([201cf31](https://github.com/contriboss/vein/commit/201cf316fc16faf527e4b9419a9848cf227a745c))
* **ci:** add explicit rustup target add ([01a4712](https://github.com/contriboss/vein/commit/01a4712060f1cfc80b46d1732bbd063c40c417ac))
* **ci:** add release concurrency control ([bb241da](https://github.com/contriboss/vein/commit/bb241dabc2d7c54b297893a7657ff0c542fa49c7))
* **ci:** filter artifacts to vein-* pattern only ([db59317](https://github.com/contriboss/vein/commit/db59317bc7450abd1aac07d8f30abe34df6eb8e2))
* **ci:** FreeBSD 15.0 + cmake ([1fab6aa](https://github.com/contriboss/vein/commit/1fab6aa23541e256dd930724c018a92af689323c))
* **ci:** FreeBSD llvm18 + LIBCLANG_PATH, resilient upload-assets ([2c57cc0](https://github.com/contriboss/vein/commit/2c57cc0437ba3e1aec723149e65516a09fded8b3))
* **ci:** use macos-15-intel instead of retired macos-13 ([fc02270](https://github.com/contriboss/vein/commit/fc022703323336586fe485d5a5ad5a5a3b204bb9))
* switch to published rama crate and integrate chrono-machines backoff ([7ade4db](https://github.com/contriboss/vein/commit/7ade4dbeaabceddb9b3d45aa1e57e42fc9021541))
* update syntax to rust 2024. ([0779d2e](https://github.com/contriboss/vein/commit/0779d2e9b7563e19a7e24589383a898c4d8da185))

## [0.3.0](https://github.com/contriboss/vein/compare/v0.2.0...v0.3.0) (2025-12-23)


### Features

* add FreeBSD build and switch Linux to musl ([52f4f12](https://github.com/contriboss/vein/commit/52f4f1205f4b5a299cfd7bdee5dd719ba949dc97))


### Bug Fixes

* resolve PR compatibility issues ([ebfcdce](https://github.com/contriboss/vein/commit/ebfcdcef32c6d8943f9a9586bfa51d81d1f55663))

## [0.2.0](https://github.com/contriboss/vein/compare/v0.1.0...v0.2.0) (2025-12-16)


### Features

* add release-please workflow with linked workspace versioning ([61b178b](https://github.com/contriboss/vein/commit/61b178b63645acf3e968033a5bd4821dd6d8b1a8))
* migrate quarantine feature to vein ([a5b99fb](https://github.com/contriboss/vein/commit/a5b99fb945c5df1bc99b5d2ead50b6cab6aff551))
* port Android support ([74cc752](https://github.com/contriboss/vein/commit/74cc7523c4fb8ee3ebd8b85dd2012426067aa915))
