//! Query builders used to express SELECT, INSERT, UPDATE, and DELETE
//! statements against an [`Entity`](crate::EntityTrait).
//!
//! Most queries originate from an entity, through [`EntityTrait::find`](crate::EntityTrait::find)
//! and its siblings.
//!
//! The builders implement composable traits — [`QueryFilter`],
//! [`QuerySelect`], [`QueryOrder`], [`QueryTrait`] — for adding `WHERE`
//! clauses, joins, ordering, and column projections. Once composed, hand the
//! query to a [`ConnectionTrait`] via `.one(db)`, `.all(db)`, `.exec(db)`,
//! etc.
//!
//! For raw SQL, build a [`Statement`] with
//! [`Statement::from_sql_and_values`].

mod debug;
mod delete;
mod helper;
mod insert;
mod select;
mod traits;
mod update;
mod util;

pub use debug::*;
pub use delete::*;
pub use helper::*;
pub use insert::*;
pub use select::*;
pub use traits::*;
pub use update::*;
pub(crate) use util::*;

pub use crate::{
    ConnectionTrait, InsertResult, PaginatorTrait, SelectExt, Statement, TransactionTrait,
    UpdateResult, Value, Values,
};
pub use sql_builder::ExprTrait;
