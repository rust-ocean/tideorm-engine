//! Exercise the statement contracts through engine-owned types.
//! This test-only spelling adapter does not replace TideORM's production facade.

extern crate self as tideorm;

pub mod orm {
    pub use tideorm_engine::{DbBackend, Statement, StatementBuilder, Value, Values, sql_builder};
}

#[path = "support/statement_contracts.rs"]
mod contracts;
