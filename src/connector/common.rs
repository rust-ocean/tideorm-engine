use crate::{ConnAcquireErr, ConnectOptions, DbErr, RuntimeErr};

/// Converts an [driver::error] execution error to a [DbErr]
pub fn driver_error_to_exec_err(err: driver::Error) -> DbErr {
    DbErr::Exec(RuntimeErr::DriverError(err.into()))
}

/// Converts an [driver::error] query error to a [DbErr]
pub fn driver_error_to_query_err(err: driver::Error) -> DbErr {
    DbErr::Query(RuntimeErr::DriverError(err.into()))
}

/// Converts an [driver::error] connection error to a [DbErr]
pub fn driver_error_to_conn_err(err: driver::Error) -> DbErr {
    DbErr::Conn(RuntimeErr::DriverError(err.into()))
}

/// The driver's options for `url`, or an error quoting `url`, its
/// credentials masked, with the driver's reason.
pub(crate) fn parse_url<O>(url: &str) -> Result<O, DbErr>
where
    O: std::str::FromStr<Err = driver::Error>,
{
    url.parse().map_err(|error| {
        crate::error::conn_err(format!(
            "The connection string '{}' cannot be parsed: {error}",
            crate::mask_url_credentials(url)
        ))
    })
}

/// Converts an [driver::error] error to a [DbErr]
pub fn driver_conn_acquire_err(driver_err: driver::Error) -> DbErr {
    match driver_err {
        driver::Error::PoolTimedOut => DbErr::ConnectionAcquire(ConnAcquireErr::Timeout),
        driver::Error::PoolClosed => DbErr::ConnectionAcquire(ConnAcquireErr::ConnectionClosed),
        _ => DbErr::Conn(RuntimeErr::DriverError(driver_err.into())),
    }
}

/// `sql` as a driver query with the arguments the engine's binder encoded.
///
/// A value the driver cannot encode fails the query when it runs, as
/// `driver::Error::Encode` and before anything reaches the server: on the
/// connection the statement was given, inside its metric callback, mapped to a
/// `DbErr` like any other failure of that statement.
pub(crate) fn driver_query_with<DB, A>(
    sql: &str,
    arguments: Result<A, driver::Error>,
) -> driver::query::Query<'_, DB, A>
where
    DB: driver::Database,
    A: driver::IntoArguments<DB>,
{
    let arguments = arguments.map_err(|error| match error {
        driver::Error::Encode(source) => source,
        other => Box::new(other) as driver_core::error::BoxDynError,
    });
    driver_core::query::query_with_result(driver::AssertSqlSafe(sql), arguments)
}

/// Opens a pool on `connect` with `options`' pool settings, connecting now
/// unless `options` ask for a lazy pool.
pub(crate) async fn open_pool<DB: super::Driver>(
    options: &ConnectOptions,
    connect: <DB::Connection as driver::Connection>::Options,
) -> Result<super::DriverPoolConnection<DB>, DbErr> {
    let pool_options = options.driver_pool_options();
    let pool = if options.connect_lazy {
        pool_options.connect_lazy_with(connect)
    } else {
        pool_options
            .connect_with(connect)
            .await
            .map_err(driver_error_to_conn_err)?
    };
    Ok(pool.into())
}

/// How long a pooled connection sits idle before a checkout pings it, unless
/// [`ConnectOptions::test_before_acquire_if_idle_for`] sets another: a
/// connection in use is let through without a round trip, one idle long
/// enough to have been dropped by the server, a proxy or a firewall is tested.
const IDLE_PING_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(1);

impl ConnectOptions {
    /// Convert [ConnectOptions] into [driver::pool::PoolOptions]
    pub fn driver_pool_options<DB>(&self) -> driver::pool::PoolOptions<DB>
    where
        DB: driver::Database,
    {
        let mut opt = driver::pool::PoolOptions::new();
        if let Some(max_connections) = self.max_connections {
            opt = opt.max_connections(max_connections);
        }
        if let Some(min_connections) = self.min_connections {
            opt = opt.min_connections(min_connections);
        }
        if let Some(timeout) = self.connect_timeout {
            opt = opt.connect_timeout(timeout);
        }
        if let Some(timeout) = self.acquire_timeout {
            opt = opt.acquire_timeout(timeout);
        }
        if let Some(idle_timeout) = self.idle_timeout {
            opt = opt.idle_timeout(idle_timeout);
        }
        if let Some(max_lifetime) = self.max_lifetime {
            opt = opt.max_lifetime(max_lifetime);
        }
        opt = opt
            .test_before_acquire(self.test_before_acquire)
            .test_before_acquire_if_idle_for(
                self.test_before_acquire_if_idle_for
                    .or(self.test_before_acquire.then_some(IDLE_PING_THRESHOLD)),
            );
        opt
    }
}
