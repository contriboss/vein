-- Every name crates.io and npm publish, for search when the registry is out
-- of reach. Gems keep their catalog in `catalog_gems`.
CREATE TABLE registry_names (
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    latest_version TEXT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (kind, name)
);

-- Prefix matches (`name LIKE 'serde%'`) without a scan of npm's millions.
CREATE INDEX idx_registry_names_prefix ON registry_names (kind, name text_pattern_ops);
