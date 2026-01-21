# Changelog

## [0.5.0](https://github.com/contriboss/vein/compare/v0.4.0...v0.5.0) (2026-01-21)


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

* address all 14 Copilot PR review issues (NULL comparisons, platform parsing, error handling, tests, code quality) ([d69b3e2](https://github.com/contriboss/vein/commit/d69b3e2c539105196fc6c0dfade5265d6dda99f2))
* address clippy warnings ([966671a](https://github.com/contriboss/vein/commit/966671a53dd68adbc21e7d54a2fb8f15e8a46f9b))
* address copilot review concerns ([2737931](https://github.com/contriboss/vein/commit/2737931ca52328508bf6802344fc7e800b1bb01c))

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
