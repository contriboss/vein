# Vein 💎

A fast, intelligent multi-ecosystem package proxy/cache for RubyGems, crates.io, and npm.

Docs, quickstart, configuration reference, and deployment guides: **https://vein.contriboss.com**

## Why

Using Vein results in faster, cheaper, more reliable CI builds. Once a package
has been fetched, it is served from your own cache instead of the public
registry. This speeds up builds, cuts outbound bandwidth, and keeps builds
working when rubygems.org, crates.io or npm is slow or down.

You also get supply-chain protection through the release delay. You can refuse
to serve a package version until it has been public for, say, 7 days. Most
malicious npm or gem releases, like hijacked maintainer accounts or typosquats,
are caught and yanked within hours or days. A delay keeps your builds from
pulling them in that window.

Vein generates CycloneDX SBOMs and keeps a catalog, so you get a central
record of which packages and versions your organization actually pulls. That
helps with compliance (SOC 2, the EU Cyber Resilience Act, etc) and with
answering “are we affected?” when a new CVE drops.

The exposed MCP resources, and optional GitHits integration for code and docs
context make Vein agent friendly. Agents can then look up package metadata and
docs through the same trusted proxy your builds use.

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
