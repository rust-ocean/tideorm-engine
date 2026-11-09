extern crate self as tideorm;
pub mod orm {
    pub use tideorm_engine::*;
}
#[path = "support/entity_foundations.rs"]
mod contracts;
