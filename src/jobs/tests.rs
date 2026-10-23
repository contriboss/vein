use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions};

use vein_adapter::CacheBackendTrait;

use super::*;

#[test]
fn the_config_decides_the_schedules() {
    let tasks = |config: &Config| -> Vec<(String, &'static str)> {
        recurring(config)
            .into_iter()
            .map(|job| (job.key, job.task))
            .collect()
    };
    let default = Config::default();
    assert_eq!(
        tasks(&default),
        [("vein:catalog:gem".to_owned(), "catalog.sync.gem")]
    );

    let mut harbor = Config::default();
    harbor.catalog.registries = vec![AssetKind::Gem, AssetKind::Crate, AssetKind::NpmPackage];
    harbor.delay_policy.enabled = true;
    assert_eq!(
        tasks(&harbor),
        [
            ("vein:catalog:gem".to_owned(), "catalog.sync.gem"),
            ("vein:catalog:crate".to_owned(), "catalog.sync.crate"),
            ("vein:catalog:npm".to_owned(), "catalog.sync.npm"),
            ("vein:delay:promote".to_owned(), "delay.promote"),
        ]
    );
}

/// A stampeded queue on a schema of its own in the test database, and the
/// database's url.
async fn queue() -> (Queue, String) {
    // vein-adapter's test sweeper drops `vein_test_<secs>_…` schemas once stale.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let url = std::env::var(vein_adapter::test_support::DATABASE_URL_ENV)
        .expect("set VEIN_TEST_DATABASE_URL to a Postgres the tests may create schemas in");
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let secs = jiff::Timestamp::now().as_second();
    let schema = format!("vein_test_{secs}_{}_jobs{n}", std::process::id());
    let admin = PgPool::connect(&url).await.unwrap();
    // A generated name, not input.
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |conn, _| {
            let schema = schema.clone();
            Box::pin(async move {
                sqlx::query(AssertSqlSafe(format!("SET search_path TO {schema}")))
                    .execute(conn)
                    .await
                    .map(drop)
            })
        })
        .connect(&url)
        .await
        .unwrap();
    stampeded::schema::migrate(&pool).await.unwrap();
    let queue = Queue::new(pool);
    queue.create_queue(QUEUE).await.unwrap();
    (queue, url)
}

async fn schedule_keys(queue: &Queue) -> Vec<String> {
    sqlx::query_scalar("SELECT key FROM stampeded_schedules ORDER BY key")
        .fetch_all(queue.pool())
        .await
        .unwrap()
}

/// vein keeps its own schedules in step with the config, and leaves any
/// other app's in the same database alone.
#[tokio::test]
async fn schedules_follow_the_config_and_spare_other_apps() {
    let (queue, _) = queue().await;
    queue
        .upsert_cron_schedule(
            "billing:nightly",
            "billing",
            "invoice",
            "0 3 * * *",
            "UTC",
            None,
        )
        .await
        .unwrap();
    let mut config = Config::default();
    config.catalog.registries = vec![AssetKind::Gem, AssetKind::NpmPackage];
    config.delay_policy.enabled = true;
    keep_schedules(&queue, &recurring(&config)).await.unwrap();
    assert_eq!(
        schedule_keys(&queue).await,
        [
            "billing:nightly",
            "vein:catalog:gem",
            "vein:catalog:npm",
            "vein:delay:promote"
        ]
    );

    // npm dropped from the catalog, the delay turned off.
    config.catalog.registries = vec![AssetKind::Gem];
    config.delay_policy.enabled = false;
    keep_schedules(&queue, &recurring(&config)).await.unwrap();
    assert_eq!(
        schedule_keys(&queue).await,
        ["billing:nightly", "vein:catalog:gem"]
    );
}

/// Two syncs of one registry would fight over its checkpoint: while one
/// holds the registry's lock, another stands down (here, before any
/// network: the lock is taken first).
#[tokio::test]
async fn a_second_sync_of_a_registry_stands_down() {
    let (_, url) = queue().await;
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let mut holder = url
        .parse::<PgConnectOptions>()
        .unwrap()
        .connect()
        .await
        .unwrap();
    let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(lock_key(AssetKind::NpmPackage))
        .fetch_one(&mut holder)
        .await
        .unwrap();
    assert!(held);

    sync_exclusively(&url, &index, AssetKind::NpmPackage)
        .await
        .unwrap();
    assert_eq!(
        index.names_count(AssetKind::NpmPackage).await.unwrap(),
        0,
        "nothing synced"
    );
}
