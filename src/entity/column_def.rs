/// SQL column types.
pub use sql_builder::ColumnType;

/// A column's SQL type and nullability, as an entity declares them.
///
/// Returned by [`ColumnTrait::def`](crate::ColumnTrait::def); reads and binds
/// go by its type. DDL is not rendered from entities.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub(crate) col_type: ColumnType,
    pub(crate) null: bool,
}

impl ColumnDef {
    /// Mark the column as nullable
    pub fn null(self) -> Self {
        self.nullable()
    }

    /// Mark the column as nullable
    pub fn nullable(mut self) -> Self {
        self.null = true;
        self
    }

    /// Get [ColumnType] as reference
    pub fn get_column_type(&self) -> &ColumnType {
        &self.col_type
    }

    /// Returns true if the column is nullable
    pub fn is_null(&self) -> bool {
        self.null
    }
}
