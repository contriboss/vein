# Vein 💎

A fast, intelligent RubyGems proxy/mirror server. Part of the ore ecosystem alongside [ore-light](https://github.com/contriboss/ore-light).

## What is Vein?

Vein is a **smart caching proxy** for RubyGems that:

- Proxies rubygems.org with local caching
- Serves gems from local cache when available
- Only fetches from upstream on cache miss
- Built on Rama (modular service framework)
- Zero configuration - just works™

## Why Vein?

- **Blazing Fast**: High-performance proxy with Rama
- **Smart Caching**: Proxy once, serve forever from local cache
- **Supply Chain Protection**: Optional quarantine delays for new gem versions
- **Minimal Config**: Works out of the box, configure only what you need
- **Simple Deployment**: Single binary, no complex dependencies
- **Ore Integration**: Works seamlessly with ore-light's fallback mechanism

## Quick Start

```bash
# Run with Docker
docker run -p 8346:8346 -v vein-data:/data ghcr.io/contriboss/vein:latest

# Or with docker-compose
curl -O https://raw.githubusercontent.com/contriboss/vein/master/docker-compose.yml
docker-compose up -d

# Or build from source
cargo build --release
./target/release/vein serve
```

## How It Works

```
Client Request → Vein → Local Cache?
                     ├─ Hit  → Serve from filesystem
                     └─ Miss → Fetch from rubygems.org
                                 ├─ Cache locally
                                 └─ Serve to client
```

**Permanent Caching**: Once a gem is cached, it's served locally forever. No re-fetching.

**Simple Architecture**: SQLite for metadata + filesystem for gem files.

## Features

- [x] Rama-based HTTP proxy
- [x] SQLite gem inventory (persistent metadata)
- [x] Filesystem storage (`./gems/`)
- [x] Smart cache resolver
- [x] Stream-through caching (cache while serving)
- [x] SHA256 verification
- [x] Minimal configuration
- [x] Docker image
- [x] Gem name/version/platform parsing
- [x] Request logging with metrics
- [x] Cache revalidation on corruption
- [x] Legacy API rejection (with monitoring)
- [x] CycloneDX SBOM extraction with admin preview & download API
- [x] Quarantine system (supply chain attack protection)

### Usage

```bash
# (Optional) write a config file – defaults are similar to this snippet
cat <<'TOML' > vein.toml
[server]
host = "0.0.0.0"
port = 8346

[upstream]
url = "https://rubygems.org"

[storage]
path = "./gems"

[database]
path = "./vein.db"
TOML

# Start the proxy (streams uncached gems through Rama)
cargo run -- serve --config vein.toml

# Inspect cache statistics
cargo run -- stats --config vein.toml
```

### CycloneDX SBOM access

- **Admin dashboard**: start `make admin` then browse to `http://127.0.0.1:9400/catalog/<gem>?version=<version>` to preview the generated SBOM and download the JSON directly from the UI.
- **Proxy endpoint**: any client can fetch the SBOM without the admin UI by calling `GET /.well-known/vein/sbom?name=<gem>&version=<version>[&platform=<platform>]` against the running Vein proxy. The response is a CycloneDX 1.5 document with `Content-Type: application/json` and a download-friendly filename. Omit the `platform` query for default `ruby` builds; supply it for native variants (e.g. `arm64-darwin`).
- SBOMs are generated automatically the first time a gem is cached and refreshed whenever the gem is re-fetched.

### Quarantine (Supply Chain Protection)

Vein can delay new gem versions from appearing in Bundler's index, giving the community time to catch malicious packages before they reach your CI/CD.

**How it works:**
- New gem versions are quarantined for a configurable period (default: 3 days)
- `bundle update` and `bundle outdated` won't see quarantined versions
- Direct installs (`gem install foo -v 1.2.3`) still work (explicit choice)
- Versions auto-promote when quarantine expires

**Real-world scenario (rest-client 1.6.13, August 2019):**
- Malicious version published, yanked ~12 hours later
- Any CI/CD running `bundle update` during that window got compromised
- With Vein's 3-day quarantine: zero exposure

**Enable in config:**

```toml
[delay_policy]
enabled = true
default_delay_days = 3
skip_weekends = true        # Don't release on Sat/Sun
release_hour_utc = 10       # Release at 10:00 UTC

# Per-gem overrides (glob patterns supported)
[[delay_policy.gems]]
pattern = "rails*"
delay_days = 7              # Extra scrutiny for Rails ecosystem

[[delay_policy.gems]]
pattern = "internal-*"
delay_days = 0              # Trust internal gems

# Pin specific versions for immediate availability
[[delay_policy.pinned]]
name = "rails"
version = "8.0.1"
reason = "Security patch - verified safe"
```

**CLI commands:**

```bash
# Show quarantine status
vein quarantine status

# List versions in quarantine
vein quarantine list

# Manually promote expired versions
vein quarantine promote

# Approve a version for immediate release
vein quarantine approve rails 8.0.1 --reason "Security patch"

# Block a malicious version
vein quarantine block badgem 1.0.0 --reason "Malware detected"
```

**Admin UI:** Browse to `/quarantine` on the admin server to view stats and approve/block versions.

### Configuration

Minimal config (most settings have sensible defaults):

```toml
# vein.toml
[server]
port = 8346  # default

[upstream]
url = "https://rubygems.org"  # default

[storage]
path = "./gems"  # default
```

Full config options:

```toml
[server]
host = "0.0.0.0"
port = 8346
threads = 4  # Rama worker threads

[upstream]
url = "https://rubygems.org"
timeout_secs = 30
connection_pool_size = 100

[storage]
path = "./gems"

[database]
path = "vein.db"  # SQLite inventory

[logging]
level = "info"  # debug, info, warn, error

[hotcache]
# Automatic refresh schedule (cron format: "sec min hour day month weekday year")
refresh_schedule = "0 0 * * * *"  # Every hour (default)
# refresh_schedule = "0 */30 * * * *"  # Every 30 minutes
# refresh_schedule = ""  # Disabled

[delay_policy]
enabled = false              # Enable quarantine system
default_delay_days = 3       # Default quarantine period
skip_weekends = true         # Don't release on weekends
release_hour_utc = 10        # Hour to release (0-23)
```

### Storage Architecture

Vein uses a **dual-database architecture** for optimal performance:

#### SQLite (`vein.db`) - Persistent Metadata Store
**Purpose**: Authoritative source of truth for all cached gems

**Stores**:
- Full gem metadata (name, version, platform)
- Filesystem paths
- SHA256 checksums
- File sizes
- Last accessed timestamps

**When Used**:
- On cache misses to verify if gem needs fetching
- On gem cache to store metadata

## Development

```bash
# Build (SQLite backend, default)
cargo build --release

# Build with PostgreSQL backend
cargo build --release --no-default-features --features postgres,tls

# Run (with logging)
RUST_LOG=debug cargo run -- serve

# Test
cargo test
```

**Note:** SQLite and PostgreSQL are mutually exclusive at compile time. Pick one.

## Docker Deployment

### Basic Usage

```bash
# Pull the image
docker pull ghcr.io/contriboss/vein:latest

# Run with persistent volumes
docker run -d \
  --name vein \
  -p 8346:8346 \
  -v vein-gems:/data/gems \
  -v vein-db:/data/db \
  -e RUST_LOG=info \
  ghcr.io/contriboss/vein:latest

# View logs
docker logs -f vein
```

### Using Docker Compose

```bash
# Start the service
docker-compose up -d

# View logs
docker-compose logs -f

# Stop the service
docker-compose down
```

### Custom Configuration

```bash
# Create config file
cp vein.example.toml vein.toml
# Edit as needed...

# Run with custom config
docker run -d \
  --name vein \
  -p 8346:8346 \
  -v $(pwd)/vein.toml:/data/vein.toml:ro \
  -v vein-gems:/data/gems \
  -v vein-db:/data/db \
  vein:latest serve --config /data/vein.toml
```

## Deployment

### Systemd Service

```ini
[Unit]
Description=Vein RubyGems Proxy
After=network.target

[Service]
Type=simple
User=vein
ExecStart=/usr/local/bin/vein serve --config /etc/vein/vein.toml
Restart=always

[Install]
WantedBy=multi-user.target
```

### Behind Nginx

```nginx
upstream vein {
    server localhost:8346;
}

server {
    listen 443 ssl http2;
    server_name gems.company.com;

    location / {
        proxy_pass http://vein;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

## Relationship to ore-light

- **ore-light**: Go-based Bundler alternative (client-side gem management)
- **Vein**: RubyGems proxy/server (server-side gem hosting)

Together they provide a complete, modern Ruby dependency management ecosystem:
- ore-light handles dependency resolution and installation
- Vein provides fast, cached gem delivery

## Why Rama (and not Pingora)

Vein was initially built on Cloudflare's Pingora framework. However, **FreeBSD support is completely lacking** in Pingora, and contributions to fix this are ignored. A PR to add FreeBSD support received no response.

So I migrated to [Rama](https://github.com/plabayo/rama), which:
- Compiles flawlessly on FreeBSD 15.0-STABLE (and Linux, macOS, Windows, iOS, Android)
- Has truly modular architecture (not "modular" in name only)
- Welcomes contributions from individuals, not just large companies
- Doesn't lock you into opinionated patterns that force forks when you step outside boundaries

Rama is a healthier choice for projects that need flexibility and multi-platform support.

Thanks, Cloudflare.

## Commercial Use & Extensions

Vein is built on [Rama](https://github.com/plabayo/rama), a modular service framework developed by [Plabayo](https://plabayo.tech).

**Project Status**: Vein is a **side project** and will remain free and open source. It is not commercialized.

**HTTP Features**: Intentionally basic. Vein does what it needs to do: proxy, cache, serve gems. No plans to add complex HTTP features or enterprise-grade capabilities.

**Need More?** Companies requiring additional features (advanced routing, auth, monitoring, protocol extensions) should **hire Plabayo directly** to extend Vein:
- Extensions can be public (contributed upstream) or private (internal forks)
- This keeps Vein focused and sends work to the team that built the foundation (Rama)
- Avoids turning the author into a full-time proxy consultant

**Support Contracts**: Plabayo offers commercial service contracts for Rama-based projects. Contact them at https://plabayo.tech

## License

Vein is dual-licensed:

- **Individual/Personal Use**: MIT License (see [LICENSE-MIT](./LICENSE-MIT))
- **Commercial/Company Use**: AGPL-3.0 (see [LICENSE-AGPL](./LICENSE-AGPL))

You may choose the license that best suits your use case. If using within a commercial organization, AGPL-3.0 terms apply.
