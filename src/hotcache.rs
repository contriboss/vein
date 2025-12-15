use std::cmp::Ordering;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::CacheBackend;
use crate::config::{BackoffStrategy, ReliabilityConfig};
use anyhow::{Context, Result};
use parking_lot::RwLock;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use semver::Version;
use tracing::{debug, warn};

// redb table: key = "gem_name:version", value = (exists: bool, is_latest: bool)
const GEM_CACHE: TableDefinition<'_, &str, (bool, bool)> = TableDefinition::new("gem_cache");

/// Hot cache for fast gem existence and latest-version lookups
///
/// This is a memory-mapped redb database that provides microsecond lookups for:
/// - Does gem X version Y exist in our cache?
/// - Is this the latest version?
///
/// Refreshed periodically from SQLite and upstream metadata.
#[derive(Clone)]
pub struct HotCache {
    db: Arc<RwLock<Database>>,
    reliability: ReliabilityConfig,
}

impl HotCache {
    /// Open or create the hot cache database
    #[allow(dead_code)]
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_config(path, ReliabilityConfig::default())
    }

    /// Open or create the hot cache database with custom reliability config
    pub fn open_with_config(path: &Path, reliability: ReliabilityConfig) -> Result<Self> {
        let db = Database::create(path)
            .with_context(|| format!("opening hot cache at {}", path.display()))?;

        // Initialize table
        let write_txn = db.begin_write()?;
        {
            let _table = write_txn.open_table(GEM_CACHE)?;
        }
        write_txn.commit()?;

        Ok(Self {
            db: Arc::new(RwLock::new(db)),
            reliability,
        })
    }

    /// Check if a gem exists in cache and if it's the latest version
    ///
    /// Returns: Some((exists, is_latest)) or None if not in hot cache
    pub fn get(&self, name: &str, version: &str) -> Result<Option<(bool, bool)>> {
        let key = format!("{}:{}", name, version);
        let db = self.db.read();
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(GEM_CACHE)?;

        Ok(table.get(key.as_str())?.map(|v| v.value()))
    }

    /// Mark a gem as existing in cache
    pub fn set(&self, name: &str, version: &str, exists: bool, is_latest: bool) -> Result<()> {
        let key = format!("{}:{}", name, version);
        let db = self.db.write();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(GEM_CACHE)?;
            table.insert(key.as_str(), (exists, is_latest))?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Bulk insert multiple gems (for refresh operations)
    pub fn bulk_insert(&self, entries: Vec<(String, String, bool, bool)>) -> Result<()> {
        let db = self.db.write();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(GEM_CACHE)?;
            for (name, version, exists, is_latest) in entries {
                let key = format!("{}:{}", name, version);
                table.insert(key.as_str(), (exists, is_latest))?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Clear the entire hot cache (for full refresh)
    pub fn clear(&self) -> Result<()> {
        let db = self.db.write();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(GEM_CACHE)?;
            // Delete all entries
            let keys: Vec<String> = table
                .iter()?
                .filter_map(|r| r.ok())
                .map(|(k, _)| k.value().to_string())
                .collect();

            for key in keys {
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get statistics about the hot cache
    pub fn stats(&self) -> Result<HotCacheStats> {
        let db = self.db.read();
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(GEM_CACHE)?;

        let mut total = 0;
        let mut exists_count = 0;
        let mut latest_count = 0;

        for (_, value) in (table.iter()?).flatten() {
            let (exists, is_latest) = value.value();
            total += 1;
            if exists {
                exists_count += 1;
            }
            if is_latest {
                latest_count += 1;
            }
        }

        Ok(HotCacheStats {
            total_entries: total,
            cached_gems: exists_count,
            latest_versions: latest_count,
        })
    }

    /// Refresh hot cache from SQLite index
    ///
    /// This updates the "exists" and "is_latest" flags based on what's in the cache.
    /// Call this periodically to keep the hot cache in sync.
    pub async fn refresh_from_index<B>(&self, index: &B) -> Result<()>
    where
        B: CacheBackend + ?Sized,
    {
        use std::collections::HashMap;

        // Get all cached gems from SQLite
        let gems = index.get_all_gems().await?;

        // Group by gem name and find latest version for each
        let mut latest_versions: HashMap<String, String> = HashMap::new();

        for (name, version) in &gems {
            match latest_versions.get(name) {
                Some(current_latest) => {
                    // Use proper version comparison that handles semver and pre-releases
                    if compare_gem_versions(version, current_latest) == std::cmp::Ordering::Greater
                    {
                        latest_versions.insert(name.clone(), version.clone());
                    }
                }
                None => {
                    latest_versions.insert(name.clone(), version.clone());
                }
            }
        }

        // Build entries for bulk insert
        let mut entries = Vec::new();
        for (name, version) in gems {
            let is_latest = latest_versions.get(&name) == Some(&version);
            entries.push((name, version, true, is_latest));
        }

        // Reset before bulk update to avoid stale rows (with retry)
        self.clear_with_retry()?;

        // Bulk update hot cache (with retry)
        if !entries.is_empty() {
            let count = entries.len();
            self.bulk_insert_with_retry(entries)?;
            tracing::info!(count = count, "Hot cache refreshed from SQLite index");
        }

        Ok(())
    }

    /// Clear the entire hot cache with retry logic
    fn clear_with_retry(&self) -> Result<()> {
        self.execute_with_retry("clear", || self.clear())
    }

    /// Bulk insert with retry logic
    fn bulk_insert_with_retry(&self, entries: Vec<(String, String, bool, bool)>) -> Result<()> {
        // Clone entries for retry attempts
        let entries_arc = Arc::new(entries);

        self.execute_with_retry("bulk_insert", || {
            self.bulk_insert(entries_arc.as_ref().clone())
        })
    }

    /// Execute a cache operation with retry logic
    fn execute_with_retry<F>(&self, operation: &str, mut operation_fn: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let retry_config = &self.reliability.retry;

        if !retry_config.enabled {
            debug!(operation, "Retry disabled for cache operation");
            return operation_fn();
        }

        let mut attempt = 0;
        let mut backoff_ms = retry_config.initial_backoff_ms;
        let max_backoff_ms = retry_config.max_backoff_secs * 1000;

        loop {
            attempt += 1;

            match operation_fn() {
                Ok(()) => {
                    if attempt > 1 {
                        tracing::info!(
                            attempts = attempt,
                            operation,
                            "Hot cache operation succeeded after retry"
                        );
                    }
                    return Ok(());
                }
                Err(err) => {
                    // Check if error is retryable
                    if !is_cache_retryable_error(&err) {
                        tracing::error!(
                            attempts = attempt,
                            operation,
                            error = %err,
                            "Hot cache operation failed with non-retryable error"
                        );
                        return Err(err);
                    }

                    // Check if we've exhausted retries
                    if attempt >= retry_config.max_attempts {
                        tracing::error!(
                            attempts = attempt,
                            operation,
                            error = %err,
                            "Hot cache operation failed after max retries"
                        );
                        return Err(err);
                    }

                    // Log retry attempt
                    warn!(
                        attempt,
                        max_attempts = retry_config.max_attempts,
                        backoff_ms,
                        operation,
                        error = %err,
                        "Hot cache operation failed, retrying"
                    );

                    // Wait before retry (blocking sleep since this is sync)
                    std::thread::sleep(Duration::from_millis(backoff_ms));

                    // Calculate next backoff
                    backoff_ms = match retry_config.backoff_strategy {
                        BackoffStrategy::Exponential => (backoff_ms * 2).min(max_backoff_ms),
                        BackoffStrategy::Fibonacci => {
                            ((backoff_ms as f64 * 1.618) as u64).min(max_backoff_ms)
                        }
                        BackoffStrategy::Constant => retry_config.initial_backoff_ms,
                    };
                }
            }
        }
    }
}

/// Determine if a cache error is retryable
fn is_cache_retryable_error(err: &anyhow::Error) -> bool {
    let err_str = err.to_string().to_lowercase();

    // Non-retryable errors (invalid data, logic errors)
    if err_str.contains("invalid")
        || err_str.contains("malformed")
        || err_str.contains("corrupt")
        || err_str.contains("does not exist")
        || err_str.contains("permission denied")
    {
        debug!(error = %err, "Non-retryable cache error detected");
        return false;
    }

    // Retryable errors (lock contention, I/O errors, transient failures)
    if err_str.contains("lock")
        || err_str.contains("busy")
        || err_str.contains("timeout")
        || err_str.contains("would block")
        || err_str.contains("i/o error")
        || err_str.contains("io error")
        || err_str.contains("resource temporarily unavailable")
    {
        debug!(error = %err, "Retryable cache error detected");
        return true;
    }

    // Default: assume retryable for unknown errors
    debug!(error = %err, "Unknown cache error type, treating as retryable");
    true
}

/// Compare two RubyGems version strings
/// Returns Ordering::Greater if v1 > v2, Ordering::Less if v1 < v2, Ordering::Equal if v1 == v2
fn compare_gem_versions(v1: &str, v2: &str) -> Ordering {
    // Try parsing as semver first
    if let (Ok(ver1), Ok(ver2)) = (Version::parse(v1), Version::parse(v2)) {
        return ver1.cmp(&ver2);
    }

    // Handle RubyGems-style versions (which may not be strict semver)
    // Split into numeric and prerelease parts
    let parts1: Vec<&str> = v1.split('.').collect();
    let parts2: Vec<&str> = v2.split('.').collect();

    // Compare numeric parts
    for i in 0..parts1.len().max(parts2.len()) {
        let p1 = parts1
            .get(i)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let p2 = parts2
            .get(i)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        match p1.cmp(&p2) {
            Ordering::Equal => continue,
            other => return other,
        }
    }

    // If all numeric parts are equal, check for pre-release identifiers
    // Pre-releases come before normal versions (e.g., "1.0.0.pre" < "1.0.0")
    let has_pre1 =
        v1.contains("pre") || v1.contains("rc") || v1.contains("beta") || v1.contains("alpha");
    let has_pre2 =
        v2.contains("pre") || v2.contains("rc") || v2.contains("beta") || v2.contains("alpha");

    match (has_pre1, has_pre2) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => v1.cmp(v2), // Fall back to string comparison for same prerelease status
    }
}

#[derive(Debug, Clone)]
pub struct HotCacheStats {
    pub total_entries: usize,
    pub cached_gems: usize,
    pub latest_versions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use vein_adapter::{AssetKey, AssetKind, SqliteCacheBackend};

    fn create_test_cache() -> (HotCache, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_cache.redb");
        let cache = HotCache::open(&db_path).unwrap();
        (cache, temp_dir)
    }

    #[test]
    fn test_cache_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("init_test.redb");

        // Should successfully create a new cache
        let result = HotCache::open(&db_path);
        assert!(result.is_ok());

        // Cache file should exist
        assert!(db_path.exists());

        // Note: redb doesn't support reopening the same file in the same process
        // This is expected behavior for memory-mapped databases
    }

    #[test]
    fn test_get_empty_cache() {
        let (cache, _temp) = create_test_cache();

        // Getting from empty cache should return None
        let result = cache.get("capistrano", "3.19.2").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_set_and_get_single_entry() {
        let (cache, _temp) = create_test_cache();

        // Set a gem entry
        cache.set("kamal", "2.9.0", true, true).unwrap();

        // Should be able to retrieve it
        let result = cache.get("kamal", "2.9.0").unwrap();
        assert_eq!(result, Some((true, true)));
    }

    #[test]
    fn test_set_and_get_multiple_versions() {
        let (cache, _temp) = create_test_cache();

        // Set multiple versions of the same gem
        cache.set("state_machines", "0.50.0", true, false).unwrap();
        cache.set("state_machines", "0.100.0", true, false).unwrap();
        cache.set("state_machines", "0.100.4", true, true).unwrap();

        // Each version should maintain its own state
        assert_eq!(
            cache.get("state_machines", "0.50.0").unwrap(),
            Some((true, false))
        );
        assert_eq!(
            cache.get("state_machines", "0.100.0").unwrap(),
            Some((true, false))
        );
        assert_eq!(
            cache.get("state_machines", "0.100.4").unwrap(),
            Some((true, true))
        );
    }

    #[test]
    fn test_set_exists_false() {
        let (cache, _temp) = create_test_cache();

        // Test marking a gem as non-existent
        cache.set("nonexistent", "1.0.0", false, false).unwrap();

        let result = cache.get("nonexistent", "1.0.0").unwrap();
        assert_eq!(result, Some((false, false)));
    }

    #[test]
    fn test_update_existing_entry() {
        let (cache, _temp) = create_test_cache();

        // Set initial value
        cache.set("capistrano", "3.19.2", true, true).unwrap();
        assert_eq!(
            cache.get("capistrano", "3.19.2").unwrap(),
            Some((true, true))
        );

        // Update to mark as not latest
        cache.set("capistrano", "3.19.2", true, false).unwrap();
        assert_eq!(
            cache.get("capistrano", "3.19.2").unwrap(),
            Some((true, false))
        );
    }

    #[test]
    fn test_bulk_insert_empty() {
        let (cache, _temp) = create_test_cache();

        // Empty bulk insert should succeed
        let result = cache.bulk_insert(vec![]);
        assert!(result.is_ok());

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);
    }

    #[test]
    fn test_bulk_insert_single() {
        let (cache, _temp) = create_test_cache();

        let entries = vec![("kamal".to_string(), "2.9.0".to_string(), true, true)];

        cache.bulk_insert(entries).unwrap();

        assert_eq!(cache.get("kamal", "2.9.0").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_bulk_insert_multiple() {
        let (cache, _temp) = create_test_cache();

        let entries = vec![
            ("capistrano".to_string(), "3.18.0".to_string(), true, false),
            ("capistrano".to_string(), "3.19.1".to_string(), true, false),
            ("capistrano".to_string(), "3.19.2".to_string(), true, true),
            ("kamal".to_string(), "2.8.2".to_string(), true, true),
            ("pg".to_string(), "1.5.9".to_string(), true, false),
            ("pg".to_string(), "1.6.2".to_string(), true, true),
        ];

        cache.bulk_insert(entries).unwrap();

        // Verify all entries
        assert_eq!(
            cache.get("capistrano", "3.18.0").unwrap(),
            Some((true, false))
        );
        assert_eq!(
            cache.get("capistrano", "3.19.2").unwrap(),
            Some((true, true))
        );
        assert_eq!(cache.get("kamal", "2.8.2").unwrap(), Some((true, true)));
        assert_eq!(cache.get("pg", "1.5.9").unwrap(), Some((true, false)));
        assert_eq!(cache.get("pg", "1.6.2").unwrap(), Some((true, true)));

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 6);
    }

    #[test]
    fn test_clear_empty_cache() {
        let (cache, _temp) = create_test_cache();

        // Clearing empty cache should succeed
        let result = cache.clear();
        assert!(result.is_ok());

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);
    }

    #[test]
    fn test_clear_populated_cache() {
        let (cache, _temp) = create_test_cache();

        // Add some entries
        cache.set("capistrano", "3.19.2", true, true).unwrap();
        cache.set("kamal", "2.8.2", true, true).unwrap();
        cache.set("rack", "3.0.0", true, true).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 3);

        // Clear the cache
        cache.clear().unwrap();

        // Should be empty now
        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);

        // Previous entries should not exist
        assert_eq!(cache.get("capistrano", "3.19.2").unwrap(), None);
        assert_eq!(cache.get("kamal", "2.8.2").unwrap(), None);
    }

    #[test]
    fn test_stats_empty_cache() {
        let (cache, _temp) = create_test_cache();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.cached_gems, 0);
        assert_eq!(stats.latest_versions, 0);
    }

    #[test]
    fn test_stats_with_exists_flags() {
        let (cache, _temp) = create_test_cache();

        // Mix of exists=true and exists=false
        cache.set("capistrano", "3.19.2", true, true).unwrap();
        cache.set("kamal", "2.8.2", true, false).unwrap();
        cache.set("nonexistent", "1.0.0", false, false).unwrap();
        cache.set("rack", "3.0.0", true, true).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 4);
        assert_eq!(stats.cached_gems, 3); // Only exists=true
        assert_eq!(stats.latest_versions, 2); // Only is_latest=true
    }

    #[test]
    fn test_stats_all_latest() {
        let (cache, _temp) = create_test_cache();

        cache.set("capistrano", "3.19.2", true, true).unwrap();
        cache.set("kamal", "2.8.2", true, true).unwrap();
        cache.set("rack", "3.0.0", true, true).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.cached_gems, 3);
        assert_eq!(stats.latest_versions, 3);
    }

    #[test]
    fn test_stats_none_latest() {
        let (cache, _temp) = create_test_cache();

        cache.set("capistrano", "3.18.0", true, false).unwrap();
        cache.set("kamal", "2.7.0", true, false).unwrap();
        cache.set("rack", "2.0.0", true, false).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.cached_gems, 3);
        assert_eq!(stats.latest_versions, 0);
    }

    #[test]
    fn test_key_format() {
        let (cache, _temp) = create_test_cache();

        // Test various gem name and version formats
        cache.set("my-gem", "1.0.0", true, true).unwrap();
        cache.set("my_gem", "2.0.0", true, true).unwrap();
        cache.set("MyGem", "3.0.0", true, true).unwrap();
        cache
            .set("state_machines", "0.100.0.alpha", true, true)
            .unwrap();
        cache.set("rack", "3.0.0.rc1", true, true).unwrap();

        // All should be retrievable
        assert_eq!(cache.get("my-gem", "1.0.0").unwrap(), Some((true, true)));
        assert_eq!(cache.get("my_gem", "2.0.0").unwrap(), Some((true, true)));
        assert_eq!(cache.get("MyGem", "3.0.0").unwrap(), Some((true, true)));
        assert_eq!(
            cache.get("state_machines", "0.100.0.alpha").unwrap(),
            Some((true, true))
        );
        assert_eq!(cache.get("rack", "3.0.0.rc1").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_key_collision_resistance() {
        let (cache, _temp) = create_test_cache();

        // Test that different gem/version combinations create different keys
        // "gem_a" + "1.0" vs "gem" + "a_1.0" should be different
        cache.set("gem_a", "1.0", true, true).unwrap();
        cache.set("gem", "a_1.0", true, false).unwrap();

        // Each should maintain its own state
        assert_eq!(cache.get("gem_a", "1.0").unwrap(), Some((true, true)));
        assert_eq!(cache.get("gem", "a_1.0").unwrap(), Some((true, false)));
    }

    #[test]
    fn test_clone_cache() {
        let (cache, _temp) = create_test_cache();

        cache.set("capistrano", "3.19.2", true, true).unwrap();

        // Clone should share the same underlying database
        let cache_clone = cache.clone();

        // Can read from clone
        assert_eq!(
            cache_clone.get("capistrano", "3.19.2").unwrap(),
            Some((true, true))
        );

        // Writes through clone are visible
        cache_clone.set("sinatra", "3.0.0", true, true).unwrap();
        assert_eq!(cache.get("sinatra", "3.0.0").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_special_characters_in_name() {
        let (cache, _temp) = create_test_cache();

        // Test gem names with special characters
        cache
            .set("activerecord-import", "1.0.0", true, true)
            .unwrap();
        cache.set("nokogiri", "1.13.0", true, true).unwrap();
        cache.set("aws-sdk-s3", "1.140.0", true, true).unwrap();

        assert_eq!(
            cache.get("activerecord-import", "1.0.0").unwrap(),
            Some((true, true))
        );
        assert_eq!(cache.get("nokogiri", "1.13.0").unwrap(), Some((true, true)));
        assert_eq!(
            cache.get("aws-sdk-s3", "1.140.0").unwrap(),
            Some((true, true))
        );
    }

    #[test]
    fn test_version_with_special_characters() {
        let (cache, _temp) = create_test_cache();

        // Test version strings with various formats
        cache.set("gem1", "1.0.0.alpha.1", true, true).unwrap();
        cache.set("gem2", "2.0.0-rc1", true, true).unwrap();
        cache.set("gem3", "3.0.0.pre.beta", true, true).unwrap();

        assert_eq!(
            cache.get("gem1", "1.0.0.alpha.1").unwrap(),
            Some((true, true))
        );
        assert_eq!(cache.get("gem2", "2.0.0-rc1").unwrap(), Some((true, true)));
        assert_eq!(
            cache.get("gem3", "3.0.0.pre.beta").unwrap(),
            Some((true, true))
        );
    }

    #[test]
    fn test_large_bulk_insert() {
        let (cache, _temp) = create_test_cache();

        // Create 100 gem entries
        let mut entries = Vec::new();
        for i in 0..100 {
            entries.push((
                format!("gem{}", i),
                "1.0.0".to_string(),
                true,
                i == 99, // Only last one is latest
            ));
        }

        cache.bulk_insert(entries).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 100);
        assert_eq!(stats.cached_gems, 100);
        assert_eq!(stats.latest_versions, 1);

        // Verify first and last
        assert_eq!(cache.get("gem0", "1.0.0").unwrap(), Some((true, false)));
        assert_eq!(cache.get("gem99", "1.0.0").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_clear_and_repopulate() {
        let (cache, _temp) = create_test_cache();

        // First population
        cache.set("capistrano", "3.18.1", true, true).unwrap();
        cache.set("kamal", "2.8.2", true, true).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 2);

        // Clear
        cache.clear().unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);

        // Repopulate with different data
        cache.set("rack", "3.0.0", true, true).unwrap();
        cache.set("puma", "5.0.0", true, true).unwrap();
        cache.set("webrick", "1.7.0", true, true).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 3);

        // Old data should not exist
        assert_eq!(cache.get("capistrano", "3.18.1").unwrap(), None);
        assert_eq!(cache.get("kamal", "2.8.2").unwrap(), None);

        // New data should exist
        assert_eq!(cache.get("rack", "3.0.0").unwrap(), Some((true, true)));
        assert_eq!(cache.get("puma", "5.0.0").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_empty_gem_name() {
        let (cache, _temp) = create_test_cache();

        // Empty gem name should still work (edge case)
        cache.set("", "1.0.0", true, true).unwrap();
        assert_eq!(cache.get("", "1.0.0").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_empty_version() {
        let (cache, _temp) = create_test_cache();

        // Empty version should still work (edge case)
        cache.set("capistrano", "", true, true).unwrap();
        assert_eq!(cache.get("capistrano", "").unwrap(), Some((true, true)));
    }

    #[test]
    fn test_both_empty() {
        let (cache, _temp) = create_test_cache();

        // Both empty
        cache.set("", "", true, true).unwrap();
        assert_eq!(cache.get("", "").unwrap(), Some((true, true)));
    }

    #[tokio::test]
    async fn test_refresh_from_index_empty() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.redb");

        let cache = HotCache::open(&cache_path).unwrap();

        // Use in-memory database for testing (matches db.rs test pattern)
        let index = SqliteCacheBackend::connect_memory().await.unwrap();

        // Refresh from empty index
        cache.refresh_from_index(&index).await.unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);
    }

    #[tokio::test]
    async fn test_refresh_from_index_single_gem() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.redb");

        let cache = HotCache::open(&cache_path).unwrap();
        let index = SqliteCacheBackend::connect_memory().await.unwrap();

        // Add a gem to the index
        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "capistrano",
            version: "3.19.2",
            platform: None,
        };
        index
            .insert_or_replace(&key, "/tmp/capistrano-3.19.2.gem", "abc123", 1024)
            .await
            .unwrap();

        // Refresh hot cache
        cache.refresh_from_index(&index).await.unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.cached_gems, 1);
        assert_eq!(stats.latest_versions, 1);

        // Verify the entry
        assert_eq!(
            cache.get("capistrano", "3.19.2").unwrap(),
            Some((true, true))
        );
    }

    #[tokio::test]
    async fn test_refresh_from_index_multiple_versions() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.redb");

        let cache = HotCache::open(&cache_path).unwrap();
        let index = SqliteCacheBackend::connect_memory().await.unwrap();

        // Add multiple versions of state_machines
        for version in &["0.50.0", "0.100.0", "0.100.4"] {
            let key = AssetKey {
                kind: AssetKind::Gem,
                name: "state_machines",
                version,
                platform: None,
            };
            index
                .insert_or_replace(
                    &key,
                    &format!("/tmp/state_machines-{}.gem", version),
                    "abc123",
                    1024,
                )
                .await
                .unwrap();
        }

        // Refresh hot cache
        cache.refresh_from_index(&index).await.unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.cached_gems, 3);
        assert_eq!(stats.latest_versions, 1); // Only 0.100.4 should be latest

        // Verify entries
        assert_eq!(
            cache.get("state_machines", "0.50.0").unwrap(),
            Some((true, false))
        );
        assert_eq!(
            cache.get("state_machines", "0.100.0").unwrap(),
            Some((true, false))
        );
        assert_eq!(
            cache.get("state_machines", "0.100.4").unwrap(),
            Some((true, true))
        );
    }

    #[tokio::test]
    async fn test_refresh_from_index_multiple_gems() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.redb");

        let cache = HotCache::open(&cache_path).unwrap();
        let index = SqliteCacheBackend::connect_memory().await.unwrap();

        // Add multiple gems with multiple versions
        let gems = vec![
            ("capistrano", vec!["3.18.0", "3.19.2"]),
            ("kamal", vec!["2.7.0", "2.9.0"]),
            ("pg", vec!["1.5.9", "1.6.2"]),
        ];

        for (name, versions) in &gems {
            for version in versions {
                let key = AssetKey {
                    kind: AssetKind::Gem,
                    name,
                    version,
                    platform: None,
                };
                index
                    .insert_or_replace(
                        &key,
                        &format!("/tmp/{}-{}.gem", name, version),
                        "abc123",
                        1024,
                    )
                    .await
                    .unwrap();
            }
        }

        // Refresh hot cache
        cache.refresh_from_index(&index).await.unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 6);
        assert_eq!(stats.cached_gems, 6);
        assert_eq!(stats.latest_versions, 3); // One latest per gem

        // Verify latest versions
        assert_eq!(
            cache.get("capistrano", "3.19.2").unwrap(),
            Some((true, true))
        );
        assert_eq!(cache.get("kamal", "2.9.0").unwrap(), Some((true, true)));
        assert_eq!(cache.get("pg", "1.6.2").unwrap(), Some((true, true)));

        // Verify non-latest versions
        assert_eq!(
            cache.get("capistrano", "3.18.0").unwrap(),
            Some((true, false))
        );
        assert_eq!(cache.get("kamal", "2.7.0").unwrap(), Some((true, false)));
        assert_eq!(cache.get("pg", "1.5.9").unwrap(), Some((true, false)));
    }

    #[tokio::test]
    async fn test_refresh_clears_old_entries() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.redb");

        let cache = HotCache::open(&cache_path).unwrap();
        let index = SqliteCacheBackend::connect_memory().await.unwrap();

        // Add initial data to hot cache
        cache.set("old_gem", "1.0.0", true, true).unwrap();
        cache.set("capistrano", "3.18.0", true, true).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 2);

        // Add only capistrano 3.19.2 to index
        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "capistrano",
            version: "3.19.2",
            platform: None,
        };
        index
            .insert_or_replace(&key, "/tmp/capistrano-3.19.2.gem", "abc123", 1024)
            .await
            .unwrap();

        // Refresh should clear old entries
        cache.refresh_from_index(&index).await.unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 1);

        // Old entries should be gone
        assert_eq!(cache.get("old_gem", "1.0.0").unwrap(), None);
        assert_eq!(cache.get("capistrano", "3.18.0").unwrap(), None);

        // New entry should exist
        assert_eq!(
            cache.get("capistrano", "3.19.2").unwrap(),
            Some((true, true))
        );
    }
}
