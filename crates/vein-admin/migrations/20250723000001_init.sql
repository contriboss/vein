-- vein-admin database schema
-- This is a placeholder - the admin primarily uses vein's cache_backend
-- Future: could store admin sessions, preferences, audit logs

CREATE TABLE IF NOT EXISTS admin_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
