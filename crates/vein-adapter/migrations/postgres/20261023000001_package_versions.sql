-- The release delay covers every registry, not only gems: one table of
-- tracked versions, keyed by registry kind (`gem`, `crate`, `npm`).
ALTER TABLE gem_versions RENAME TO package_versions;
ALTER TABLE package_versions ADD COLUMN kind TEXT NOT NULL DEFAULT 'gem';
ALTER TABLE package_versions ALTER COLUMN kind DROP DEFAULT;

-- An admin's block was stored as `yanked`; vein never tracked upstream yanks.
UPDATE package_versions SET status = 'blocked' WHERE status = 'yanked';
ALTER TABLE package_versions DROP COLUMN upstream_yanked;
ALTER TABLE package_versions DROP COLUMN sha256;

-- The old key let NULL platforms repeat; keep the newest row of each.
DELETE FROM package_versions older
USING package_versions newer
WHERE older.name = newer.name
  AND older.version = newer.version
  AND older.platform IS NOT DISTINCT FROM newer.platform
  AND older.kind = newer.kind
  AND older.id < newer.id;
ALTER TABLE package_versions DROP CONSTRAINT gem_versions_unique;
ALTER TABLE package_versions
    ADD CONSTRAINT package_versions_unique UNIQUE NULLS NOT DISTINCT (kind, name, version, platform);

DROP INDEX idx_gem_versions_name;
CREATE INDEX idx_package_versions_kind_name ON package_versions(kind, name);
ALTER INDEX idx_gem_versions_status RENAME TO idx_package_versions_status;
ALTER INDEX idx_gv_available RENAME TO idx_package_versions_available;
