# Vein 💎

A fast, intelligent multi-ecosystem package proxy/cache for RubyGems, crates.io, and npm.

Docs, quickstart, configuration reference, and deployment guides: **https://vein.contriboss.com**

## Choosing registries

Each registry is a Cargo feature, all three on by default: `gems`, `crates`, `npm`.
Build only the ones you serve:

```sh
cargo build --release --no-default-features --features tls,admin,crates
```

A registry left out has no routes, search, docs, catalog, release delay or MCP
resources: its requests get a 404 saying which feature is missing, and a config
naming it (`[upstream]` for gems, `[catalog] registries`, delay entries) is refused.
Without `gems`, no Ruby parser, SBOM or YAML code is built; without `crates`, no zstd.

`githits` (default) builds the [GitHits](https://githits.com) code context provider: MCP
tools over public open-source code and docs, off until `[code_context] provider = "githits"`.

## License

BSD-3-Clause. See [LICENSE](./LICENSE).
