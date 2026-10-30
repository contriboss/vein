-- Public API symbols extracted from crates, gems, npm packages
CREATE TABLE package_exports (
    id BIGSERIAL PRIMARY KEY,
    registry TEXT NOT NULL,           -- 'crate' | 'gem' | 'npm'
    name TEXT NOT NULL,               -- package name
    version TEXT NOT NULL,            -- semver
    symbol TEXT NOT NULL,             -- function/struct/class name
    kind TEXT NOT NULL,               -- 'function'|'struct'|'enum'|'trait'|'module'|'type'|'const'|'macro'|'reexport'
    path TEXT NOT NULL,               -- fully qualified path (tokio::spawn)
    docs TEXT,                        -- optional doc string
    source_file TEXT,                 -- where it's defined/exported
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (registry, name, version, symbol)
);

-- Fast lookup: list all exports for a version
CREATE INDEX idx_package_exports_version ON package_exports (registry, name, version);

-- Fast lookup: all versions of a package
CREATE INDEX idx_package_exports_name ON package_exports (registry, name);

-- Search by symbol path
CREATE INDEX idx_package_exports_path ON package_exports (path);
