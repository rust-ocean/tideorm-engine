//! TideORM's internal execution engine.
//!
//! TideORM re-exports this crate, with the derive macros from `tideorm-macros`,
//! as `tideorm::orm`.
#![warn(missing_docs)]
#![deny(unsafe_code)]
#![deny(
    missing_debug_implementations,
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::print_stdout
)]

mod connector;
mod database;
pub mod entity;
/// Error types returned by engine operations.
pub mod error;
mod executor;
/// Per-query metric collection hooks.
pub mod metric;
pub mod query;
#[doc(hidden)]
pub use sql_builder::lexer as sql;
pub mod value;

/// Takes `format!`-style arguments and expands to a no-op that evaluates none of them:
/// the engine's debug output is compiled out, and executed statements reach the
/// [`metric`] callback instead.
#[macro_export]
macro_rules! debug_print {
    ($( $args:expr ),*) => {
        true;
    };
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod binding;

#[allow(unused_imports)]
pub use connector::*;
pub use database::*;
pub use entity::*;
pub use error::*;
pub use executor::*;
pub use query::*;

pub use sql_builder;
pub use sql_builder::Iden;

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub use driver;
