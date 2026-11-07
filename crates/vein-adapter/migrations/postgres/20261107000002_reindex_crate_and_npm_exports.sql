-- Crate docs kept raw comment markers and plain comments, crate paths used
-- the package name instead of the Rust one, and some npm summaries came
-- from code examples or tag text. Read crates and npm again.
DELETE FROM package_exports_indexed WHERE registry IN ('crate', 'npm');
