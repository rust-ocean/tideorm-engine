//! Run the unchanged baseline field-state fixture through engine-owned values.

extern crate self as tideorm;

pub mod orm {
    pub use tideorm_engine::*;
}

#[path = "support/active_value_contracts.rs"]
mod contracts;
