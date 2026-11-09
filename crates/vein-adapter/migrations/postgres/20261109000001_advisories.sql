-- Security advisories for the packages vein serves, imported from GitHub's
-- reviewed advisory database (OSV format). Read when an agent looks at a
-- package through /mcp; nothing is pushed anywhere.
CREATE TABLE advisories (
    id TEXT PRIMARY KEY,
    aliases TEXT[] NOT NULL DEFAULT '{}',
    severity TEXT,
    summary TEXT,
    published TIMESTAMPTZ,
    modified TIMESTAMPTZ NOT NULL
);

-- One row per package an advisory affects: its affected intervals and any
-- versions it lists one by one, as JSON.
CREATE TABLE advisory_packages (
    advisory_id TEXT NOT NULL REFERENCES advisories (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    affected JSONB NOT NULL,
    PRIMARY KEY (advisory_id, kind, name)
);
CREATE INDEX advisory_packages_package ON advisory_packages (kind, name);

-- The advisory database commit last imported.
CREATE TABLE advisory_sync (
    source TEXT PRIMARY KEY,
    revision TEXT NOT NULL,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
