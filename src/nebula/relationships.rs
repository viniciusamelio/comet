use std::marker::PhantomData;

use super::Entity;
use super::column::Column;
use super::query::Select;
use super::rls::{AccessContext, CustomPredicateProvider, NoCustomPredicates, RlsError};
use super::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BelongsTo<Child, Parent, T> {
    local_column: Column<T>,
    foreign_column: Column<T>,
    _child: PhantomData<Child>,
    _parent: PhantomData<Parent>,
}

impl<Child, Parent, T> BelongsTo<Child, Parent, T> {
    pub const fn new(local_column: Column<T>, foreign_column: Column<T>) -> Self {
        Self {
            local_column,
            foreign_column,
            _child: PhantomData,
            _parent: PhantomData,
        }
    }

    pub fn local_column(&self) -> Column<T> {
        Column::new(self.local_column.table, self.local_column.name)
    }

    pub fn foreign_column(&self) -> Column<T> {
        Column::new(self.foreign_column.table, self.foreign_column.name)
    }
}

impl<Child, Parent, T> BelongsTo<Child, Parent, T>
where
    Parent: Entity,
{
    pub fn select_parent<V>(&self, local_value: V) -> Select<Parent>
    where
        V: Into<Value>,
    {
        Parent::select()
            .where_(self.foreign_column().eq(local_value))
            .limit(1)
    }

    pub fn select_parent_scoped<V>(
        &self,
        local_value: V,
        context: &AccessContext,
    ) -> Result<Select<Parent>, RlsError>
    where
        V: Into<Value>,
    {
        self.select_parent_scoped_with(local_value, context, &NoCustomPredicates)
    }

    pub fn select_parent_scoped_with<V>(
        &self,
        local_value: V,
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Select<Parent>, RlsError>
    where
        V: Into<Value>,
    {
        self.select_parent(local_value)
            .apply_rls(context, predicates)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HasMany<Parent, Child, T> {
    parent_column: Column<T>,
    child_column: Column<T>,
    _parent: PhantomData<Parent>,
    _child: PhantomData<Child>,
}

impl<Parent, Child, T> HasMany<Parent, Child, T> {
    pub const fn new(parent_column: Column<T>, child_column: Column<T>) -> Self {
        Self {
            parent_column,
            child_column,
            _parent: PhantomData,
            _child: PhantomData,
        }
    }

    pub fn parent_column(&self) -> Column<T> {
        Column::new(self.parent_column.table, self.parent_column.name)
    }

    pub fn child_column(&self) -> Column<T> {
        Column::new(self.child_column.table, self.child_column.name)
    }
}

impl<Parent, Child, T> HasMany<Parent, Child, T>
where
    Child: Entity,
{
    pub fn select_children<V>(&self, parent_value: V) -> Select<Child>
    where
        V: Into<Value>,
    {
        Child::select().where_(self.child_column().eq(parent_value))
    }

    pub fn select_children_scoped<V>(
        &self,
        parent_value: V,
        context: &AccessContext,
    ) -> Result<Select<Child>, RlsError>
    where
        V: Into<Value>,
    {
        self.select_children_scoped_with(parent_value, context, &NoCustomPredicates)
    }

    pub fn select_children_scoped_with<V>(
        &self,
        parent_value: V,
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Select<Child>, RlsError>
    where
        V: Into<Value>,
    {
        self.select_children(parent_value)
            .apply_rls(context, predicates)
    }
}

pub const fn belongs_to<Child, Parent, T>(
    local_column: Column<T>,
    foreign_column: Column<T>,
) -> BelongsTo<Child, Parent, T> {
    BelongsTo::new(local_column, foreign_column)
}

pub const fn has_many<Parent, Child, T>(
    parent_column: Column<T>,
    child_column: Column<T>,
) -> HasMany<Parent, Child, T> {
    HasMany::new(parent_column, child_column)
}
