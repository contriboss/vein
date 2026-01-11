-- Vein SQLite Schema

CREATE TABLE cached_assets (
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    platform TEXT,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    last_accessed TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (kind, name, version, platform)
);

CREATE TABLE catalog_gems (
    name TEXT PRIMARY KEY,
    latest_version TEXT,
    synced_at TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
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
    has_native_extensions INTEGER NOT NULL DEFAULT 0,
    has_embedded_binaries INTEGER NOT NULL DEFAULT 0,
    required_ruby_version TEXT,
    required_rubygems_version TEXT,
    rubygems_version TEXT,
    specification_version INTEGER,
    built_at TEXT,
    size_bytes INTEGER,
    sha256 TEXT,
    sbom_json TEXT,
    PRIMARY KEY (name, version, platform)
);

CREATE TABLE gem_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    platform TEXT,
    sha256 TEXT,
    published_at TIMESTAMP NOT NULL,
    available_after TIMESTAMP NOT NULL,
    status TEXT NOT NULL DEFAULT 'quarantine',
    status_reason TEXT,
    upstream_yanked INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE (name, version, platform)
);

CREATE INDEX idx_gem_versions_name ON gem_versions(name);
CREATE INDEX idx_gem_versions_status ON gem_versions(status);
CREATE INDEX idx_gv_available ON gem_versions(available_after);

CREATE TABLE gem_symbols (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    gem_name TEXT NOT NULL,
    gem_version TEXT NOT NULL,
    gem_platform TEXT,
    file_path TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    symbol_name TEXT NOT NULL,
    parent_name TEXT,
    line_number INTEGER,
    created_at TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(gem_name, gem_version, gem_platform, file_path, symbol_name)
);

CREATE INDEX idx_gem_symbols_name ON gem_symbols(symbol_name);
CREATE INDEX idx_gem_symbols_gem ON gem_symbols(gem_name, gem_version);
