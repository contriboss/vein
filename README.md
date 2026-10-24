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

## License

Vein is dual-licensed:

- **Individual/personal use**: MIT License (see [LICENSE-MIT](./LICENSE-MIT))
- **Commercial/company use**: AGPL-3.0 (see [LICENSE-AGPL](./LICENSE-AGPL))

You may choose the license that best suits your use case. If using within a commercial organization, AGPL-3.0 terms apply.
