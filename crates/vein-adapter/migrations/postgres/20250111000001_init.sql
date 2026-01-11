-- Vein PostgreSQL Schema

CREATE TABLE cached_assets (
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    platform TEXT,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    last_accessed TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT cached_assets_unique UNIQUE (kind, name, version, platform)
);

CREATE INDEX idx_cached_assets_kind ON cached_assets(kind);
CREATE INDEX idx_cached_assets_name ON cached_assets(name);

CREATE TABLE catalog_gems (
    name TEXT PRIMARY KEY,
    latest_version TEXT,
    synced_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE catalog_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE gem_metadata (
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    platform TEXT NOT NULL DEFAULT 'ruby',
    summary TEXT,
    description TEXT,
    licenses TEXT,
    authors TEXT,
    emails TEXT,
    homepage TEXT,
    documentation_url TEXT,
    changelog_url TEXT,
    source_code_url TEXT,
    bug_tracker_url TEXT,
    wiki_url TEXT,
    funding_url TEXT,
    metadata_json TEXT,
    dependencies_json TEXT NOT NULL DEFAULT '[]',
    executables_json TEXT,
    extensions_json TEXT,
    native_languages_json TEXT,
    has_native_extensions BOOLEAN NOT NULL DEFAULT FALSE,
    has_embedded_binaries BOOLEAN NOT NULL DEFAULT FALSE,
    required_ruby_version TEXT,
    required_rubygems_version TEXT,
    rubygems_version TEXT,
    specification_version INTEGER,
    built_at TEXT,
    size_bytes BIGINT,
    sha256 TEXT,
    sbom_json TEXT,
    PRIMARY KEY (name, version, platform)
);

CREATE TABLE gem_versions (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    platform TEXT,
    sha256 TEXT,
    published_at TIMESTAMPTZ NOT NULL,
    available_after TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'quarantine',
    status_reason TEXT,
    upstream_yanked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT gem_versions_unique UNIQUE (name, version, platform)
);

CREATE INDEX idx_gem_versions_name ON gem_versions(name);
CREATE INDEX idx_gem_versions_status ON gem_versions(status);
CREATE INDEX idx_gv_available ON gem_versions(available_after);

CREATE TABLE gem_symbols (
    id BIGSERIAL PRIMARY KEY,
    gem_name TEXT NOT NULL,
    gem_version TEXT NOT NULL,
    gem_platform TEXT NOT NULL DEFAULT 'ruby',
    file_path TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    symbol_name TEXT NOT NULL,
    parent_name TEXT,
    line_number INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT gem_symbols_unique UNIQUE (gem_name, gem_version, gem_platform, file_path, symbol_name),
    CONSTRAINT fk_gem_symbols_metadata
        FOREIGN KEY (gem_name, gem_version, gem_platform)
        REFERENCES gem_metadata(name, version, platform)
        ON DELETE CASCADE
);

CREATE INDEX idx_gem_symbols_name ON gem_symbols(symbol_name);
CREATE INDEX idx_gem_symbols_gem ON gem_symbols(gem_name, gem_version);
