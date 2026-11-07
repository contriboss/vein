-- Which cached package versions the exports index has read, so a version
-- that exports nothing (or fails to parse) is not read again every run.
CREATE TABLE package_exports_indexed (
    registry TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    export_count INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (registry, name, version)
);

-- Rows written before this ledger came from an extractor that found
-- nothing; the index starts over.
DELETE FROM package_exports;
