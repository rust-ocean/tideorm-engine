extern crate self as tideorm;
pub mod orm {
    pub use tideorm_engine::*;
}
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub mod internal {
    pub use tideorm_engine::driver;
}
include!("support/connection_options_contracts.rs");

#[cfg(all(
    feature = "runtime-tokio",
    any(feature = "postgres", feature = "mysql", feature = "sqlite")
))]
#[path = "support/pool_timeouts.rs"]
mod pool_timeouts;

#[cfg(all(feature = "runtime-tokio", feature = "postgres"))]
#[path = "support/pool_idle_ping.rs"]
mod pool_idle_ping;
