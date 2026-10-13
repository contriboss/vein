-- `GemMetadata.specification_version` is decoded as i64; INTEGER (int4)
-- fails to decode under sqlx's strict type check.
ALTER TABLE gem_metadata ALTER COLUMN specification_version TYPE BIGINT;
