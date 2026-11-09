//! Run the frozen error fixture through engine-owned types.
//! This spelling adapter is confined to this integration test.

extern crate self as tideorm;

pub mod orm {
    pub use tideorm_engine::*;
}

#[path = "support/error_contracts.rs"]
mod contracts;
