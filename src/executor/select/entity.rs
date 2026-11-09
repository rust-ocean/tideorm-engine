use super::*;

impl<E> Select<E>
where
    E: EntityTrait,
{
    /// Perform a Select operation on a Model using a [Statement]
    #[allow(clippy::wrong_self_convention)]
    pub fn from_raw_sql(self, stmt: Statement) -> SelectorRaw<SelectModel<E::Model>> {
        SelectorRaw::new(stmt)
    }

    /// Return a [Selector] from `Self` that wraps a [SelectModel]
    pub fn into_model<M>(self) -> Selector<SelectModel<M>>
    where
        M: FromQueryResult,
    {
        Selector::new(self.query)
    }

    /// Read each row as `T` (a value or a tuple), by column position.
    pub fn into_tuple<T>(self) -> Selector<SelectGetableTuple<T>>
    where
        T: TryGetableMany,
    {
        Selector::new(self.query)
    }

    /// Get one Model from the SELECT query
    pub async fn one<C>(self, db: &C) -> Result<Option<E::Model>, DbErr>
    where
        C: ConnectionTrait,
    {
        self.into_model().one(db).await
    }

    /// Get exactly one Model from the SELECT query, returning
    /// [`DbErr::RecordNotFound`] when nothing matches. The non-optional
    /// counterpart to [`one`](Self::one): use it when a missing row is an error,
    /// so the call site can use `?` instead of unwrapping an `Option`.
    pub async fn require_one<C>(self, db: &C) -> Result<E::Model, DbErr>
    where
        C: ConnectionTrait,
    {
        self.into_model().require_one(db).await
    }

    /// Get all Models from the SELECT query
    pub async fn all<C>(self, db: &C) -> Result<Vec<E::Model>, DbErr>
    where
        C: ConnectionTrait,
    {
        self.into_model().all(db).await
    }
}
