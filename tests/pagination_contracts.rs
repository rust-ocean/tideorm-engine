extern crate self as tideorm;
pub mod orm {
    pub use tideorm_engine::*;
}
#[path = "support/pagination_contracts.rs"]
mod contracts;
