extern crate self as tideorm;
pub mod orm {
    pub use tideorm_engine::*;
}
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub mod internal {
    pub use tideorm_engine::driver;
}
#[path = "support/query_contracts.rs"]
mod contracts;
