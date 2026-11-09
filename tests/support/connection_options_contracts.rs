use std::time::Duration;
use tideorm::orm::{ConnectOptions, ConnectTarget, Database, DbErr, RuntimeErr};

#[test]
fn defaults_and_debug_contract() {
    let options = ConnectOptions::new("scheme://fixture/only");
    assert!(
        matches!(options.get_target(), ConnectTarget::Url(url) if url == "scheme://fixture/only")
    );
    assert_eq!(options.get_max_connections(), None);
    assert_eq!(options.get_min_connections(), None);
    assert_eq!(options.get_connect_timeout(), None);
    assert_eq!(options.get_acquire_timeout(), None);
    assert_eq!(options.get_idle_timeout(), None);
    assert_eq!(options.get_max_lifetime(), None);
    assert!(options.get_test_before_acquire());
    assert_eq!(options.get_test_before_acquire_if_idle_for(), None);
    assert!(!options.get_connect_lazy());
    assert_eq!(options.get_statement_cache_capacity(), None);
    println!("OPTIONS_DEBUG_DEFAULT {options:?}");
}

#[test]
fn setters_clone_and_explicit_disabled_timeouts() {
    let mut options: ConnectOptions = String::from("fixture://unchanged").into();
    let pointer = &mut options as *mut ConnectOptions;
    let returned = options
        .max_connections(0)
        .min_connections(7)
        .connect_timeout(Duration::from_secs(3))
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(None)
        .max_lifetime(None)
        .test_before_acquire_if_idle_for(Duration::from_secs(11))
        .test_before_acquire(true)
        .statement_cache_capacity(0)
        .connect_lazy(true) as *mut ConnectOptions;
    assert_eq!(pointer, returned);
    let saved = options.clone();
    options
        .max_connections(9)
        .idle_timeout(Duration::from_secs(2))
        .max_lifetime(Duration::from_secs(4));
    assert_eq!(saved.get_max_connections(), Some(0)); // Validation belongs to the caller/pool.
    assert_eq!(saved.get_min_connections(), Some(7));
    assert_eq!(saved.get_idle_timeout(), Some(None));
    assert_eq!(saved.get_max_lifetime(), Some(None));
    assert_eq!(
        options.get_idle_timeout(),
        Some(Some(Duration::from_secs(2)))
    );
    assert_eq!(
        options.get_max_lifetime(),
        Some(Some(Duration::from_secs(4)))
    );
    assert_eq!(saved.get_connect_timeout(), Some(Duration::from_secs(3)));
    assert_eq!(saved.get_acquire_timeout(), Some(Duration::from_secs(5)));
    assert!(saved.get_test_before_acquire());
    assert_eq!(
        saved.get_test_before_acquire_if_idle_for(),
        Some(Duration::from_secs(11))
    );
    assert!(saved.get_connect_lazy());
    assert_eq!(saved.get_statement_cache_capacity(), Some(0));
    println!("OPTIONS_DEBUG_CONFIGURED {saved:?}");
    let borrowed: ConnectOptions = "fixture://borrowed".into();
    assert!(
        matches!(borrowed.get_target(), ConnectTarget::Url(url) if url == "fixture://borrowed")
    );
}

/// `{:?}` is how configuration ends up in logs: a URL's credentials are
/// masked and driver options, which hold the password, are left out.
#[test]
fn debug_never_shows_the_password() {
    let options = ConnectOptions::new("postgres://app:fixture-secret@db/app");
    let debug = format!("{options:?}");
    assert!(debug.contains(r#"Url("postgres://***@db/app")"#), "{debug}");
    assert!(!debug.contains("fixture-secret"), "{debug}");
    #[cfg(feature = "postgres")]
    {
        let typed = ConnectOptions::new(
            tideorm::internal::driver::postgres::PgConnectOptions::new().password("fixture-secret"),
        );
        let debug = format!("{typed:?}");
        assert!(debug.contains("target: Postgres(..)"), "{debug}");
        assert!(!debug.contains("fixture-secret"), "{debug}");
    }
}

#[test]
fn unusable_urls_are_refused() {
    async_std::task::block_on(async {
        for url in ["not a url", "fixture://local/db"] {
            let refused = Database::connect(url).await.unwrap_err();
            let expected = format!(
                "The connection string '{url}' is not a postgres://, postgresql://, mysql://, mariadb:// or sqlite: URL."
            );
            assert!(
                matches!(refused, DbErr::Conn(RuntimeErr::Internal(ref message)) if *message == expected),
                "{refused:?}"
            );
        }
        #[cfg(not(feature = "postgres"))]
        {
            let missing = Database::connect("postgres://u:secret@localhost/db")
                .await
                .unwrap_err();
            assert!(
                matches!(missing, DbErr::Conn(RuntimeErr::Internal(ref message)) if message == "The connection string 'postgres://***@localhost/db' is for PostgreSQL, whose driver this build leaves out."),
                "{missing:?}"
            );
        }
    });
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn pool_options<DB: tideorm::internal::driver::Database>() {
    let mut options = ConnectOptions::new("fixture://not-opened");
    options
        .max_connections(8)
        .min_connections(2)
        .connect_timeout(Duration::from_secs(3))
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(None)
        .max_lifetime(Duration::from_secs(13))
        .test_before_acquire_if_idle_for(Duration::from_secs(11))
        .test_before_acquire(false);
    let pool = options.driver_pool_options::<DB>();
    assert_eq!(pool.get_max_connections(), 8);
    assert_eq!(pool.get_min_connections(), 2);
    assert_eq!(pool.get_acquire_timeout(), Duration::from_secs(5));
    assert_eq!(pool.get_connect_timeout(), Duration::from_secs(3));
    assert_eq!(pool.get_idle_timeout(), None);
    assert_eq!(pool.get_max_lifetime(), Some(Duration::from_secs(13)));
    assert!(!pool.get_test_before_acquire());
    assert_eq!(
        pool.get_test_before_acquire_if_idle_for(),
        Some(Duration::from_secs(11))
    );
    let mut options = ConnectOptions::new("fixture://not-opened");
    options.connect_timeout(Duration::from_secs(3));
    assert_eq!(
        options.driver_pool_options::<DB>().get_acquire_timeout(),
        Duration::from_secs(30)
    );
    // A checkout pings a connection idle for a second, or none when disabled.
    assert_eq!(
        options
            .driver_pool_options::<DB>()
            .get_test_before_acquire_if_idle_for(),
        Some(Duration::from_secs(1))
    );
    options.test_before_acquire(false);
    assert_eq!(
        options
            .driver_pool_options::<DB>()
            .get_test_before_acquire_if_idle_for(),
        None
    );
}

/// Opens each target as a lazy pool, which dials nothing, and checks the
/// backend the URL's scheme or the driver options chose.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn opens_lazily<const N: usize>(targets: [ConnectTarget; N], backend: tideorm::orm::DbBackend) {
    let opened = async {
        for target in targets {
            let mut options = ConnectOptions::new(target.clone());
            options.connect_lazy(true);
            let db = Database::connect(options).await.unwrap();
            assert_eq!(db.get_database_backend(), backend, "{target:?}");
        }
    };
    #[cfg(feature = "runtime-tokio")]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(opened);
    #[cfg(not(feature = "runtime-tokio"))]
    async_std::task::block_on(opened);
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_options_and_schemes() {
    pool_options::<tideorm::internal::driver::Postgres>();
    opens_lazily(
        [
            "postgres://localhost/fixture".into(),
            "PostgreSQL://localhost/fixture".into(),
            tideorm::internal::driver::postgres::PgConnectOptions::new()
                .host("localhost")
                .application_name("fixture")
                .into(),
        ],
        tideorm::orm::DbBackend::Postgres,
    );
}

/// A `mariadb://` URL opens through the MySQL driver; until a connection
/// has said which server it reached, the pool answers MySQL.
#[cfg(feature = "mysql")]
#[test]
fn mysql_options_and_schemes() {
    pool_options::<tideorm::internal::driver::MySql>();
    opens_lazily(
        [
            "mysql://localhost/fixture".into(),
            "MariaDB://localhost/fixture".into(),
            tideorm::internal::driver::mysql::MySqlConnectOptions::new()
                .host("localhost")
                .into(),
        ],
        tideorm::orm::DbBackend::MySQL,
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_options_and_schemes() {
    pool_options::<tideorm::internal::driver::Sqlite>();
    opens_lazily(
        [
            "sqlite::memory:".into(),
            "SQLite::memory:".into(),
            tideorm::internal::driver::sqlite::SqliteConnectOptions::new()
                .in_memory(true)
                .into(),
        ],
        tideorm::orm::DbBackend::SQLite,
    );
}
