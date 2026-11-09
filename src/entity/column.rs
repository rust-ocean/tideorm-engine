use crate::{
    ColumnDef, ColumnType, DbBackend, EntityName, Iden, IdenStatic, IntoSimpleExpr, Iterable,
};
use sql_builder::{
    BinOper, DynIden, Expr, ExprTrait, IntoIden, IntoLikeExpr, SelectStatement, Value,
};
use std::borrow::Cow;

pub(crate) mod macros {
    macro_rules! bind_oper {
        ($vis:vis $op:ident, $bin_op:ident) => {
            #[allow(missing_docs)]
            $vis fn $op<V>(&self, v: V) -> Expr
            where
                V: Into<Value>,
            {
                let expr = self.save_enum_as(Expr::val(v));
                Expr::col(self.as_column_ref()).binary(BinOper::$bin_op, expr)
            }
        };
    }

    macro_rules! bind_func_no_params {
        ($vis:vis $func:ident) => {
            /// Applies the [`Expr`] method of the same name to this column.
            $vis fn $func(&self) -> Expr {
                Expr::col(self.as_column_ref()).$func()
            }
        };
    }

    macro_rules! bind_vec_func {
        ($vis:vis $func:ident) => {
            #[allow(missing_docs)]
            #[allow(clippy::wrong_self_convention)]
            $vis fn $func<V, I>(&self, v: I) -> Expr
            where
                V: Into<Value>,
                I: IntoIterator<Item = V>,
            {
                let v_with_enum_cast = v.into_iter().map(|v| self.save_enum_as(Expr::val(v)));
                Expr::col(self.as_column_ref()).$func(v_with_enum_cast)
            }
        };
    }

    macro_rules! bind_subquery_func {
        ($vis:vis $func:ident) => {
            #[allow(clippy::wrong_self_convention)]
            #[allow(missing_docs)]
            $vis fn $func(&self, s: SelectStatement) -> Expr {
                Expr::col(self.as_column_ref()).$func(s)
            }
        };
    }

    macro_rules! bind_array_oper {
        ($vis:vis $op:ident, $oper:ident) => {
            #[cfg(feature = "postgres")]
            /// Array operator. Postgres only.
            $vis fn $op<V, I>(&self, v: I) -> Expr
            where
                V: Into<Value> + sql_builder::ValueType + sql_builder::postgres_array::NotU8,
                I: IntoIterator<Item = V>,
            {
                use sql_builder::extension::postgres::PgBinOper;

                let vec: Vec<_> = v.into_iter().collect();
                Expr::col(self.as_column_ref()).binary(PgBinOper::$oper, self.save_enum_as(Expr::val(vec)))
            }
        };
    }

    pub(crate) use bind_array_oper;
    pub(crate) use bind_func_no_params;
    pub(crate) use bind_oper;
    pub(crate) use bind_subquery_func;
    pub(crate) use bind_vec_func;
}

use macros::*;

/// Operations and comparisons available on every entity column.
///
/// Implemented by the generated `Column` enum for each entity, this is what
/// turns `entity::COLUMN.field.eq(42)` into a filter expression. Most methods
/// (`eq`, `lt`, `like`, `is_in`, …) mirror their counterparts on
/// [`sql_builder::Expr`] but qualify the column with its table automatically.
pub trait ColumnTrait: IdenStatic + Iterable {
    /// The entity this column belongs to.
    type EntityName: EntityName;

    /// SQL type and constraints attached to this column.
    fn def(&self) -> ColumnDef;

    /// Table iden of the entity this column belongs to.
    fn entity_name(&self) -> DynIden {
        DynIden::from(Self::EntityName::default())
    }

    /// Fully-qualified `(table, column)` reference, used when building
    /// expressions that need to disambiguate columns across joined tables.
    fn as_column_ref(&self) -> (DynIden, DynIden) {
        (self.entity_name(), DynIden::from(*self))
    }

    /// Perform equality against a Value. `None` will be converted to `IS NULL`.
    fn eq<V>(&self, v: V) -> Expr
    where
        V: Into<Value>,
    {
        null_aware(
            Expr::col(self.as_column_ref()),
            BinOper::Equal,
            v.into(),
            |v| self.save_enum_as(v),
        )
    }

    /// Perform inequality against a Value. `None` will be converted to `IS NOT NULL`.
    fn ne<V>(&self, v: V) -> Expr
    where
        V: Into<Value>,
    {
        null_aware(
            Expr::col(self.as_column_ref()),
            BinOper::NotEqual,
            v.into(),
            |v| self.save_enum_as(v),
        )
    }

    bind_oper!(gt, GreaterThan);
    bind_oper!(gte, GreaterThanOrEqual);
    bind_oper!(lt, SmallerThan);
    bind_oper!(lte, SmallerThanOrEqual);

    /// `self BETWEEN a AND b`.
    fn between<V>(&self, a: V, b: V) -> Expr
    where
        V: Into<Value>,
    {
        Expr::col(self.as_column_ref()).between(a, b)
    }

    /// `self NOT BETWEEN a AND b`.
    fn not_between<V>(&self, a: V, b: V) -> Expr
    where
        V: Into<Value>,
    {
        Expr::col(self.as_column_ref()).not_between(a, b)
    }

    /// `self LIKE s`.
    fn like<T>(&self, s: T) -> Expr
    where
        T: IntoLikeExpr,
    {
        Expr::col(self.as_column_ref()).like(s)
    }

    /// `self NOT LIKE s`.
    fn not_like<T>(&self, s: T) -> Expr
    where
        T: IntoLikeExpr,
    {
        Expr::col(self.as_column_ref()).not_like(s)
    }

    /// Postgres Only.
    fn ilike<T>(&self, s: T) -> Expr
    where
        T: IntoLikeExpr,
    {
        use sql_builder::extension::postgres::PgExpr;

        Expr::col(self.as_column_ref()).ilike(s)
    }

    /// Postgres Only.
    fn not_ilike<T>(&self, s: T) -> Expr
    where
        T: IntoLikeExpr,
    {
        use sql_builder::extension::postgres::PgExpr;

        Expr::col(self.as_column_ref()).not_ilike(s)
    }

    /// This is a simplified shorthand for a more general `like` method.
    /// Use `like` if you need something more complex, like specifying an escape character.
    fn starts_with<T>(&self, s: T) -> Expr
    where
        T: Into<String>,
    {
        let pattern = format!("{}%", s.into());
        Expr::col(self.as_column_ref()).like(pattern)
    }

    /// This is a simplified shorthand for a more general `like` method.
    /// Use `like` if you need something more complex, like specifying an escape character.
    fn ends_with<T>(&self, s: T) -> Expr
    where
        T: Into<String>,
    {
        let pattern = format!("%{}", s.into());
        Expr::col(self.as_column_ref()).like(pattern)
    }

    /// This is a simplified shorthand for a more general `like` method.
    /// Use `like` if you need something more complex, like specifying an escape character.
    fn contains<T>(&self, s: T) -> Expr
    where
        T: Into<String>,
    {
        let pattern = format!("%{}%", s.into());
        Expr::col(self.as_column_ref()).like(pattern)
    }

    bind_func_no_params!(max);
    bind_func_no_params!(min);
    bind_func_no_params!(sum);
    bind_func_no_params!(avg);
    bind_func_no_params!(count);
    bind_func_no_params!(is_null);
    bind_func_no_params!(is_not_null);

    /// Provide fallback value if the column is null (null coalescing)
    fn if_null<V>(&self, v: V) -> Expr
    where
        V: Into<Value>,
    {
        Expr::col(self.as_column_ref()).if_null(v)
    }

    bind_vec_func!(is_in);
    bind_vec_func!(is_not_in);

    /// Postgres only.
    #[cfg(feature = "postgres")]
    fn eq_any<V, I>(&self, v: I) -> Expr
    where
        V: Into<Value> + sql_builder::postgres_array::NotU8,
        I: IntoIterator<Item = V>,
    {
        use sql_builder::extension::postgres::PgFunc;

        match array_operand(self, v) {
            Some(array) => Expr::col(self.as_column_ref()).eq(PgFunc::any(array)),
            None => Expr::col(self.as_column_ref()).is_in(std::iter::empty::<V>()),
        }
    }

    /// Postgres only. Opposite of `eq_any` (equivalent to `is_not_in`).
    #[cfg(feature = "postgres")]
    fn ne_all<V, I>(&self, v: I) -> Expr
    where
        V: Into<Value> + sql_builder::postgres_array::NotU8,
        I: IntoIterator<Item = V>,
    {
        use sql_builder::extension::postgres::PgFunc;

        match array_operand(self, v) {
            Some(array) => Expr::col(self.as_column_ref()).ne(PgFunc::all(array)),
            None => Expr::col(self.as_column_ref()).is_not_in(std::iter::empty::<V>()),
        }
    }

    bind_subquery_func!(in_subquery);
    bind_subquery_func!(not_in_subquery);

    bind_array_oper!(array_contains, Contains);
    bind_array_oper!(array_contained, Contained);
    bind_array_oper!(array_overlap, Overlap);

    /// Wrap the column in a plain [`Expr`], suitable for use anywhere a
    /// `sql_builder` expression is expected.
    fn into_expr(self) -> Expr {
        self.into_simple_expr()
    }

    /// Wrap the column as the expression used inside a `RETURNING` clause
    /// for the given backend.
    fn into_returning_expr(self, _db_backend: DbBackend) -> Expr {
        Expr::col(self)
    }

    /// Cast an enum column to text; no-op for non-enum columns.
    fn select_enum_as(&self, expr: Expr) -> Expr {
        cast_enum_as(expr, &self.def(), select_enum_as)
    }

    /// Cast a value into the column's enum type; no-op for non-enum columns.
    fn save_enum_as(&self, val: Expr) -> Expr {
        cast_enum_as(val, &self.def(), save_enum_as)
    }
}

/// Extension methods on [`ColumnType`] for building [`ColumnDef`]s and
/// inspecting database `ENUM` metadata.
pub trait ColumnTypeTrait {
    /// Wrap this [`ColumnType`] in a fresh [`ColumnDef`].
    fn def(self) -> ColumnDef;

    /// Name of the database `ENUM` type if this column is an enum,
    /// `None` otherwise.
    fn get_enum_name(&self) -> Option<&DynIden>;
}

impl ColumnTypeTrait for ColumnType {
    fn def(self) -> ColumnDef {
        ColumnDef {
            col_type: self,
            null: false,
        }
    }

    fn get_enum_name(&self) -> Option<&DynIden> {
        enum_name(self)
    }
}

impl ColumnTypeTrait for ColumnDef {
    fn def(self) -> ColumnDef {
        self
    }

    fn get_enum_name(&self) -> Option<&DynIden> {
        enum_name(&self.col_type)
    }
}

fn enum_name(col_type: &ColumnType) -> Option<&DynIden> {
    match col_type {
        ColumnType::Enum { name, .. } => Some(name),
        ColumnType::Array(col_type) => enum_name(col_type),
        _ => None,
    }
}

struct Text;
struct TextArray;

impl Iden for Text {
    fn quoted(&self) -> Cow<'static, str> {
        Cow::Borrowed("text")
    }

    fn unquoted(&self) -> &str {
        match self.quoted() {
            Cow::Borrowed(s) => s,
            _ => unreachable!(),
        }
    }
}

impl Iden for TextArray {
    fn quoted(&self) -> Cow<'static, str> {
        // This is Postgres only and it has a special handling for quoting this
        Cow::Borrowed("text[]")
    }

    fn unquoted(&self) -> &str {
        match self.quoted() {
            Cow::Borrowed(s) => s,
            _ => unreachable!(),
        }
    }
}

pub(crate) fn select_enum_as(col: Expr, _: DynIden, col_type: &ColumnType) -> Expr {
    let type_name = match col_type {
        ColumnType::Array(_) => TextArray.into_iden(),
        _ => Text.into_iden(),
    };
    col.as_enum(type_name)
}

pub(crate) fn save_enum_as(col: Expr, enum_name: DynIden, col_type: &ColumnType) -> Expr {
    if matches!(col, Expr::Value(Value::Enum(_))) {
        return col;
    }
    #[cfg(feature = "postgres")]
    if matches!(
        col,
        Expr::Value(Value::Array(sql_builder::ArrayType::Enum(_), _))
    ) {
        return col;
    }

    let type_name = match col_type {
        ColumnType::Array(_) => format!("{enum_name}[]").into_iden(),
        _ => enum_name,
    };
    col.as_enum(type_name)
}

/// The array operand of `eq_any` / `ne_all`: the values as one array of the
/// first one's type, cast to an array of the column's enum type (as-is for
/// other columns and typed enum arrays). `None` for no values.
#[cfg(feature = "postgres")]
fn array_operand<V>(column: &impl ColumnTrait, values: impl IntoIterator<Item = V>) -> Option<Expr>
where
    V: Into<Value>,
{
    let values: Vec<Value> = values.into_iter().map(Into::into).collect();
    let array = Value::Array(values.first()?.array_type(), Some(Box::new(values)));
    let typed = matches!(array, Value::Array(sql_builder::ArrayType::Enum(_), _));
    Some(match column.def().get_column_type().get_enum_name() {
        Some(name) if !typed => Expr::val(array).as_enum(format!("{name}[]").into_iden()),
        _ => Expr::val(array),
    })
}

/// `column op value` for `=` and `<>`, which SQL never makes true for a
/// `NULL` value: equality with one is `IS NULL`, inequality `IS NOT NULL`.
/// `save_as` writes the value as the column stores it.
pub(crate) fn null_aware(
    column: Expr,
    op: BinOper,
    value: Value,
    save_as: impl FnOnce(Expr) -> Expr,
) -> Expr {
    if value != value.as_null() {
        column.binary(op, save_as(Expr::val(value)))
    } else if op == BinOper::Equal {
        column.is_null()
    } else {
        column.is_not_null()
    }
}

pub(crate) fn cast_enum_as<F>(expr: Expr, col_def: &ColumnDef, f: F) -> Expr
where
    F: Fn(Expr, DynIden, &ColumnType) -> Expr,
{
    let col_type = col_def.get_column_type();

    match col_type {
        #[cfg(feature = "postgres")]
        ColumnType::Json | ColumnType::JsonBinary => {
            use serde_json::Value as Json;
            use sql_builder::ArrayType;

            match expr {
                Expr::Value(Value::Array(ArrayType::Json, Some(json_vec))) => {
                    // flatten Array(Vec<Json>) into Json
                    let json_vec: Vec<Json> = json_vec
                        .into_iter()
                        .filter_map(|val| match val {
                            Value::Json(Some(json)) => Some(*json),
                            _ => None,
                        })
                        .collect();
                    Expr::Value(Value::Json(Some(Box::new(json_vec.into()))))
                }
                Expr::Value(Value::Array(ArrayType::Json, None)) => Expr::Value(Value::Json(None)),
                _ => expr,
            }
        }
        _ => match col_type.get_enum_name() {
            Some(enum_name) => f(expr, enum_name.clone(), col_type),
            None => expr,
        },
    }
}
