use super::*;
use std::future::Future;

/// The editable counterpart of a [`Model`](crate::ModelTrait), used to build
/// `INSERT` and `UPDATE` statements.
///
/// Each field of an ActiveModel is wrapped in an [`ActiveValue`], so it can
/// be one of:
///
/// - [`Set(v)`](ActiveValue::Set) — a new value to write,
/// - [`Unchanged(v)`](ActiveValue::Unchanged) — the value already in the
///   database, will not be sent in `UPDATE`,
/// - [`NotSet`](ActiveValue::NotSet) — value is absent; the column is
///   omitted from `INSERT` (letting the database supply a default) and
///   from `UPDATE`.
///
/// This makes ActiveModel ideal for partial updates: only the columns you
/// touch end up in the generated `UPDATE`.
pub trait ActiveModelTrait: Clone + Debug {
    /// The [`EntityTrait`] this ActiveModel belongs to.
    type Entity: EntityTrait;

    /// Take the [`ActiveValue`] of a column, leaving it as `NotSet`.
    fn take(&mut self, c: <Self::Entity as EntityTrait>::Column) -> ActiveValue<Value>;

    /// Read the [`ActiveValue`] of a column.
    fn get(&self, c: <Self::Entity as EntityTrait>::Column) -> ActiveValue<Value>;

    /// Set one column to `Set(v)`. Panics on type mismatch; prefer
    /// [`try_set`](Self::try_set) when the value comes from untrusted input.
    fn set(&mut self, c: <Self::Entity as EntityTrait>::Column, v: Value) {
        self.try_set(c, v)
            .unwrap_or_else(|e| panic!("Failed to set value for {:?}: {e:?}", c.as_column_ref()))
    }

    /// Set one column to `Set(v)` only if `v` differs from the current value,
    /// avoiding spurious `UPDATE` rewrites. Panics on type mismatch.
    fn set_if_not_equals(&mut self, c: <Self::Entity as EntityTrait>::Column, v: Value);

    /// Set one column to `Set(v)`, returning an error on type mismatch.
    fn try_set(&mut self, c: <Self::Entity as EntityTrait>::Column, v: Value) -> Result<(), DbErr>;

    /// Mark a column as `NotSet` so it is omitted from the next `INSERT` or
    /// `UPDATE`.
    fn not_set(&mut self, c: <Self::Entity as EntityTrait>::Column);

    /// `true` if the column is currently in the `NotSet` state.
    fn is_not_set(&self, c: <Self::Entity as EntityTrait>::Column) -> bool;

    /// A fresh ActiveModel with every column `NotSet`.
    fn default() -> Self;

    /// A fresh ActiveModel pre-populated with `Set(default_value)` for each
    /// column that has a `Default` impl; other columns remain `NotSet`.
    fn default_values() -> Self;

    /// Promote one column from [`Unchanged`](ActiveValue::Unchanged) to
    /// [`Set`](ActiveValue::Set) so it will be included in the next
    /// `UPDATE`. Leaves [`NotSet`](ActiveValue::NotSet) untouched.
    fn reset(&mut self, c: <Self::Entity as EntityTrait>::Column);

    /// Apply [`reset`](Self::reset) to every column, forcing the next
    /// `UPDATE` to write all non-`NotSet` columns.
    fn reset_all(mut self) -> Self {
        for col in <Self::Entity as EntityTrait>::Column::iter() {
            self.reset(col);
        }
        self
    }

    /// The primary key as a `ValueTuple`, or `None` if any primary-key
    /// column is still `NotSet`.
    fn get_primary_key_value(&self) -> Option<ValueTuple> {
        let values = <Self::Entity as EntityTrait>::PrimaryKey::iter()
            .map(|key| self.get(key.into_column()).into_value())
            .collect::<Option<Vec<_>>>()?;
        crate::query::value_tuple(values).ok()
    }

    /// Perform an `INSERT` operation on the ActiveModel
    fn insert<C>(
        self,
        db: &C,
    ) -> impl Future<Output = Result<<Self::Entity as EntityTrait>::Model, DbErr>> + Send
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<Self>,
        C: ConnectionTrait,
        Self: Send,
    {
        async move {
            <Self::Entity as EntityTrait>::insert(self)
                .exec_with_returning(db)
                .await
        }
    }

    /// Perform the `UPDATE` operation on an ActiveModel
    fn update<C>(
        self,
        db: &C,
    ) -> impl Future<Output = Result<<Self::Entity as EntityTrait>::Model, DbErr>> + Send
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<Self>,
        C: ConnectionTrait,
        Self: Send,
    {
        async move { Self::Entity::update(self).exec(db).await }
    }

    /// Update an ActiveModel without a RETURNING clause, yielding an
    /// [`UpdateResult`] instead of the updated model. Useful on backends without
    /// RETURNING support, or when the updated model is not needed. Returns
    /// [`DbErr::RecordNotUpdated`] if no row matches.
    fn update_without_returning<C>(
        self,
        db: &C,
    ) -> impl Future<Output = Result<UpdateResult, DbErr>> + Send
    where
        C: ConnectionTrait,
        Self: Send,
    {
        async move { Self::Entity::update(self).exec_without_returning(db).await }
    }

    /// Insert the model if primary key is `NotSet`, update otherwise.
    /// Only works if the entity has auto increment primary key.
    fn save<C>(self, db: &C) -> impl Future<Output = Result<Self, DbErr>> + Send
    where
        <Self::Entity as EntityTrait>::Model: IntoActiveModel<Self>,
        C: ConnectionTrait,
        Self: Send,
    {
        async move {
            let res = if !self.is_update() {
                self.insert(db).await
            } else {
                self.update(db).await
            }?;
            Ok(res.into_active_model())
        }
    }

    /// Returns true if the primary key is fully-specified
    #[doc(hidden)]
    fn is_update(&self) -> bool {
        let mut is_update = true;
        for key in <Self::Entity as EntityTrait>::PrimaryKey::iter() {
            let col = key.into_column();
            if self.is_not_set(col) {
                is_update = false;
                break;
            }
        }
        is_update
    }

    /// Delete an active model by its primary key
    fn delete<C>(self, db: &C) -> impl Future<Output = Result<DeleteResult, DbErr>> + Send
    where
        C: ConnectionTrait,
        Self: Send,
    {
        async move { Self::Entity::delete(self).exec(db).await }
    }

    /// Set the corresponding attributes in the ActiveModel from a JSON value
    ///
    /// Note that this method will not alter the primary key values in ActiveModel.
    fn set_from_json(&mut self, json: serde_json::Value) -> Result<(), DbErr>
    where
        Self: crate::TryIntoModel<<Self::Entity as EntityTrait>::Model>,
        <<Self as ActiveModelTrait>::Entity as EntityTrait>::Model: IntoActiveModel<Self>,
        for<'de> <<Self as ActiveModelTrait>::Entity as EntityTrait>::Model:
            serde::de::Deserialize<'de> + serde::Serialize,
    {
        use crate::Iterable;

        // Backup primary key values
        let primary_key_values: Vec<(<Self::Entity as EntityTrait>::Column, ActiveValue<Value>)> =
            <<Self::Entity as EntityTrait>::PrimaryKey>::iter()
                .map(|pk| (pk.into_column(), self.take(pk.into_column())))
                .collect();

        // Replace all values in ActiveModel
        *self = Self::from_json(json)?;

        // Restore primary key values
        for (col, active_value) in primary_key_values {
            match active_value {
                ActiveValue::Unchanged(v) | ActiveValue::Set(v) => self.set(col, v),
                NotSet => self.not_set(col),
            }
        }

        Ok(())
    }

    /// Create ActiveModel from a JSON value
    fn from_json(json: serde_json::Value) -> Result<Self, DbErr>
    where
        Self: crate::TryIntoModel<<Self::Entity as EntityTrait>::Model>,
        <<Self as ActiveModelTrait>::Entity as EntityTrait>::Model: IntoActiveModel<Self>,
        for<'de> <<Self as ActiveModelTrait>::Entity as EntityTrait>::Model:
            serde::de::Deserialize<'de> + serde::Serialize,
    {
        use crate::{IdenStatic, Iterable};

        let serde_json::Value::Object(mut input) = json else {
            return Err(DbErr::Json(format!(
                "invalid type: expected JSON object for {}",
                <<Self as ActiveModelTrait>::Entity as IdenStatic>::as_str(&Default::default())
            )));
        };

        let dummy_am = Self::default_values();
        let len = <<Self::Entity as EntityTrait>::Column>::iter().len();
        // Mark down which attribute exists in the JSON object
        let mut json_keys = Vec::with_capacity(len);
        let mut merged = serde_json::Map::with_capacity(len);

        for col in <<Self::Entity as EntityTrait>::Column>::iter() {
            let key = col.as_str();
            let has_key = input.contains_key(key);
            json_keys.push((col, has_key));
            match dummy_am.get(col) {
                ActiveValue::Unchanged(value) | ActiveValue::Set(value) => {
                    merged.insert(
                        key.to_owned(),
                        sql_builder::value_to_json_value(&value)
                            .map_err(|error| DbErr::Json(error.to_string()))?,
                    );
                }
                _ => {}
            }
        }

        merged.append(&mut input);

        let json_value = serde_json::Value::Object(merged);

        // Convert JSON object into ActiveModel via Model
        let model: <Self::Entity as EntityTrait>::Model =
            serde_json::from_value(json_value).map_err(json_err)?;
        let mut am = model.into_active_model();

        // Transform attribute that exists in JSON object into ActiveValue::Set, otherwise ActiveValue::NotSet
        for (col, json_key_exists) in json_keys {
            match (json_key_exists, am.get(col)) {
                (true, ActiveValue::Set(value) | ActiveValue::Unchanged(value)) => {
                    am.set(col, value);
                }
                _ => {
                    am.not_set(col);
                }
            }
        }

        Ok(am)
    }

    /// Return `true` if any attribute of `ActiveModel` is `Set`
    fn is_changed(&self) -> bool {
        <Self::Entity as EntityTrait>::Column::iter()
            .any(|col| matches!(self.get(col), ActiveValue::Set(_)))
    }
}
