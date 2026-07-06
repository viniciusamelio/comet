use std::marker::PhantomData;

use super::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column<T> {
    pub(crate) table: &'static str,
    pub(crate) name: &'static str,
    _ty: PhantomData<T>,
}

impl<T> Column<T> {
    pub const fn new(table: &'static str, name: &'static str) -> Self {
        Self {
            table,
            name,
            _ty: PhantomData,
        }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn table(self) -> &'static str {
        self.table
    }

    pub fn eq<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("=", value)
    }

    pub fn ne<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("!=", value)
    }

    pub fn gt<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary(">", value)
    }

    pub fn gte<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary(">=", value)
    }

    pub fn lt<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("<", value)
    }

    pub fn lte<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("<=", value)
    }

    pub fn like<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("LIKE", value)
    }

    /// Matches rows whose column contains `needle`, treating `needle` as a
    /// literal substring rather than a LIKE pattern: `%`, `_`, and `\` in it
    /// are escaped so user-supplied search text can't widen or narrow the
    /// match beyond a plain substring search.
    pub fn like_escaped(self, needle: impl AsRef<str>) -> Expr {
        let pattern = format!("%{}%", escape_like_pattern(needle.as_ref()));

        Expr {
            sql: format!(
                "{} LIKE ? ESCAPE '\\'",
                qualified_column(self.table, self.name)
            ),
            binds: vec![Value::Text(pattern)],
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }

    pub fn is_null(self) -> Expr {
        Expr {
            sql: format!("{} IS NULL", qualified_column(self.table, self.name)),
            binds: Vec::new(),
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }

    pub fn is_not_null(self) -> Expr {
        Expr {
            sql: format!("{} IS NOT NULL", qualified_column(self.table, self.name)),
            binds: Vec::new(),
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }

    pub fn asc(self) -> Ordering {
        Ordering {
            table: self.table,
            column: self.name,
            direction: Direction::Asc,
        }
    }

    pub fn desc(self) -> Ordering {
        Ordering {
            table: self.table,
            column: self.name,
            direction: Direction::Desc,
        }
    }

    fn binary<V>(self, op: &'static str, value: V) -> Expr
    where
        V: Into<Value>,
    {
        Expr {
            sql: format!("{} {op} ?", qualified_column(self.table, self.name)),
            binds: vec![value.into()],
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnRef {
    pub table: &'static str,
    pub name: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub(crate) sql: String,
    pub(crate) binds: Vec<Value>,
    pub(crate) columns: Vec<ColumnRef>,
}

impl Expr {
    pub fn and(self, other: Expr) -> Expr {
        let mut binds = self.binds;
        binds.extend(other.binds);
        let mut columns = self.columns;
        columns.extend(other.columns);

        Expr {
            sql: format!("({}) AND ({})", self.sql, other.sql),
            binds,
            columns,
        }
    }

    pub fn or(self, other: Expr) -> Expr {
        let mut binds = self.binds;
        binds.extend(other.binds);
        let mut columns = self.columns;
        columns.extend(other.columns);

        Expr {
            sql: format!("({}) OR ({})", self.sql, other.sql),
            binds,
            columns,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ordering {
    pub(crate) table: &'static str,
    pub(crate) column: &'static str,
    direction: Direction,
}

pub(crate) fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Escapes `\`, `%`, and `_` so `value` matches literally inside a
/// `LIKE ... ESCAPE '\'` pattern instead of acting as SQL wildcards.
fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for character in value.chars() {
        if character == '\\' || character == '%' || character == '_' {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}

pub(crate) fn qualified_column(table: &str, column: &str) -> String {
    format!("{}.{}", quote_ident(table), quote_ident(column))
}

pub(crate) fn format_ordering(ordering: Ordering) -> String {
    let direction = match ordering.direction {
        Direction::Asc => "ASC",
        Direction::Desc => "DESC",
    };

    format!(
        "{} {direction}",
        qualified_column(ordering.table, ordering.column)
    )
}
