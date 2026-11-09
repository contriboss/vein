//! Security advisories from GitHub's advisory database, shown to agents
//! when they read a package through `/mcp`.

use serde::Deserialize;

/// GitHub's reviewed advisories, in OSV format, as a git repository.
pub const GITHUB_ADVISORY_DATABASE: &str = "https://github.com/github/advisory-database.git";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AdvisoriesConfig {
    /// Keeps a sparse clone of the advisory database under the storage
    /// path (the reviewed advisories, ~370 MB) and imports it hourly.
    /// Needs `git` on the PATH.
    pub enabled: bool,
    /// The database's git URL: GitHub's, or a mirror of it.
    pub repository: String,
}

impl Default for AdvisoriesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            repository: GITHUB_ADVISORY_DATABASE.to_owned(),
        }
    }
}
