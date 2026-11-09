//! Entities and the types and traits that describe them.
//!
//! Every database table is represented by an **Entity** — a unit
//! struct implementing [`EntityTrait`]. The Entity ties together:
//!
//! - the table's **name** (via [`EntityName`]),
//! - its **columns** (via [`ColumnTrait`]),
//! - its **primary key** (via [`PrimaryKeyTrait`] / [`PrimaryKeyToColumn`]).
//!
//! Each Entity has two companion types:
//!
//! - **[`Model`](ModelTrait)** — a plain struct mirroring a row of the table,
//!   used for reads.
//! - **[`ActiveModel`](ActiveModelTrait)** — a struct where every field is
//!   wrapped in [`ActiveValue`], used for inserts and partial updates.
//!
//! From an Entity you can build select, insert, update, and delete queries
//! via [`EntityTrait::find`], [`EntityTrait::insert`], [`EntityTrait::update`],
//! and [`EntityTrait::delete`] (plus their `_many` / `_by_id` siblings).
//!
//! TideORM's model derive generates the entity for each model. Relations are
//! TideORM's own: the engine knows tables, not how they relate.

mod active_enum;
mod active_model;
mod active_value;
mod base_entity;
pub(crate) mod column;
mod column_def;
mod identity;
mod model;
/// Re-exports the types and traits most commonly needed to define and use
/// entities. Glob-import this module in entity files.
pub mod prelude;
mod primary_key;

pub use active_enum::*;
pub use active_model::*;
pub use active_value::*;
pub use base_entity::*;
pub use column::*;
pub use column_def::*;
pub use identity::*;
pub use model::*;
pub use primary_key::*;
