-- One row per exported path: a symbol name repeats across modules
-- (`new`, `Error`), and keying on it kept only the last one written.
ALTER TABLE package_exports
    DROP CONSTRAINT IF EXISTS package_exports_registry_name_version_symbol_key;
ALTER TABLE package_exports
    ADD CONSTRAINT package_exports_registry_name_version_path_key
    UNIQUE (registry, name, version, path);
