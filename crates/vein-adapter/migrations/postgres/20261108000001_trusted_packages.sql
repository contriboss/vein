-- Packages an admin trusts at runtime: their versions skip the release
-- delay, as a `delay_days = 0` entry in [delay_policy] does, with no restart.
CREATE TABLE trusted_packages (
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    reason TEXT,
    trusted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (kind, name)
);
