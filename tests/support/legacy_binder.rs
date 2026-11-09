//! The binder TideORM used before the engine's own, which panics on some
//! values and drops others; kept only so `fallible_bindings` can compare the
//! engine's binder with it.

#[cfg(feature = "mysql")]
#[path = "legacy_binder/mysql.rs"]
mod mysql;
#[cfg(feature = "postgres")]
#[path = "legacy_binder/postgres.rs"]
mod postgres;
#[cfg(feature = "sqlite")]
#[path = "legacy_binder/sqlite.rs"]
mod sqlite;

#[derive(Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct LegacyValues(pub sql_builder::Values);
