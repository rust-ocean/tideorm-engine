//! Run the execution-result contracts through engine-owned execution results.

extern crate self as tideorm;

pub mod orm {
    pub use tideorm_engine::*;
}

#[path = "support/execution_result_contracts.rs"]
mod contracts;
