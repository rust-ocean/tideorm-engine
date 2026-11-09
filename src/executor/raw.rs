use std::fmt::Write as _;

use crate::sql::{Kind, Token, tokens};
use crate::{DbBackend, Statement, Value, Values};

/// `FROM (<sql>) AS sub_query`: the rows of the raw statement `sql` read as
/// a derived table. Its trailing comments and one statement terminator are
/// left out, since inside the parentheses they would end the outer statement.
pub(super) fn sub_query_source(sql: &str, backend: DbBackend) -> String {
    derived_table(trim_raw_statement(sql, backend))
}

fn derived_table(sql: &str) -> String {
    format!("FROM ({sql}) AS sub_query")
}

/// `SELECT <projection>` from the rows of the raw `statement`, keeping its SQL
/// and bindings opaque: its placeholders are never fed back through a query
/// builder, and no presumed SELECT is sliced off.
pub(super) fn wrap(mut statement: Statement, projection: &str) -> Statement {
    let source = sub_query_source(&statement.sql, statement.db_backend);
    statement.sql = format!("SELECT {projection} {source}");
    statement
}

/// At most `limit` rows of the raw `statement`, from the `offset`th on.
///
/// On MySQL, MariaDB and SQLite the bounds follow the query itself, which
/// keeps its column names: a derived table refuses a name two columns share
/// on the first two, and SQLite renames the second `name:1`. A query with a
/// `LIMIT`, `OFFSET` or `FETCH` of its own is read as a derived table, as it
/// always is on PostgreSQL, which keeps such names. The bounds go before a
/// row lock (`FOR UPDATE`, `LOCK IN SHARE MODE`), which MySQL takes last. The
/// limit is written and the offset bound, after the statement's own values.
pub(super) fn limited(mut statement: Statement, limit: u64, offset: Option<u64>) -> Statement {
    let backend = statement.db_backend;
    let sql = trim_raw_statement(&statement.sql, backend);
    let (mut limited, lock) = if backend == DbBackend::Postgres || bounds_its_rows(sql, backend) {
        (format!("SELECT * {} LIMIT {limit}", derived_table(sql)), "")
    } else {
        let (query, lock) = sql.split_at(locking_clause(sql, backend).unwrap_or(sql.len()));
        (format!("{} LIMIT {limit}", query.trim_end()), lock)
    };
    if let Some(offset) = offset {
        limited.push_str(" OFFSET ");
        let _ = match backend {
            DbBackend::MySQL | DbBackend::MariaDB => limited.write_char('?'),
            DbBackend::Postgres => write!(limited, "${}", next_marker(&statement)),
            DbBackend::SQLite => write!(limited, "?{}", next_marker(&statement)),
        };
        statement
            .values
            .get_or_insert_with(|| Values(Vec::new()))
            .0
            .push(Value::BigUnsigned(Some(offset)));
    }
    if !lock.is_empty() {
        limited.push(' ');
        limited.push_str(lock);
    }
    statement.sql = limited;
    statement
}

/// Whether the raw query sets a `LIMIT`, `OFFSET` or `FETCH` outside every
/// parenthesis, after which another `LIMIT` is a syntax error.
fn bounds_its_rows(sql: &str, backend: DbBackend) -> bool {
    let mut depth = 0usize;
    tokens(sql, backend.into()).any(|token| {
        if token.is_symbol(&["("]) {
            depth += 1;
        } else if token.is_symbol(&[")"]) {
            depth = depth.saturating_sub(1);
        }
        depth == 0 && token.is_word(&["LIMIT", "OFFSET", "FETCH"])
    })
}

/// Where the raw query's row lock starts, outside every parenthesis: a
/// `FOR UPDATE` or `FOR SHARE` (with its `OF`, `NOWAIT` or `SKIP LOCKED`), or
/// MySQL's `LOCK IN SHARE MODE`.
fn locking_clause(sql: &str, backend: DbBackend) -> Option<usize> {
    let mut depth = 0usize;
    let mut previous: Option<Token<'_>> = None;
    for token in tokens(sql, backend.into()) {
        if matches!(token.kind, Kind::Space | Kind::Comment) {
            continue;
        }
        if token.is_symbol(&["("]) {
            depth += 1;
        } else if token.is_symbol(&[")"]) {
            depth = depth.saturating_sub(1);
        } else if depth == 0
            && let Some(first) = previous
            && (first.is_word(&["FOR"]) && token.is_word(&["UPDATE", "SHARE"])
                || first.is_word(&["LOCK"]) && token.is_word(&["IN"]))
        {
            return Some(first.start);
        }
        previous = Some(token);
    }
    None
}

/// The number for a marker past every value `statement` binds and every
/// marker its SQL holds, so that a statement missing a value still fails
/// rather than reading the one added after them. On SQLite a bare `?` takes
/// the number after the highest one before it.
fn next_marker(statement: &Statement) -> usize {
    let sql = statement.sql.as_str();
    let backend = statement.db_backend;
    let mut highest = 0_usize;
    for token in tokens(sql, backend.into()) {
        let digits = match token.kind {
            Kind::Parameter => token.text.strip_prefix('$'),
            Kind::Symbol if token.text == "?" => {
                let rest = &sql[token.start + 1..];
                Some(&rest[..rest.bytes().take_while(u8::is_ascii_digit).count()])
            }
            _ => None,
        };
        match digits {
            Some("") if backend == DbBackend::SQLite => highest = highest.saturating_add(1),
            Some(digits) => {
                if let Ok(number) = digits.parse::<usize>() {
                    highest = highest.max(number);
                }
            }
            None => {}
        }
    }
    let bound = statement.values.as_ref().map_or(0, |values| values.0.len());
    // A marker past `usize::MAX` is the database's to refuse, not a panic.
    highest.max(bound).saturating_add(1)
}

/// Remove trailing whitespace and comments, plus one statement terminator,
/// without interpreting markers inside quoted SQL as syntax. SQL with an
/// unclosed quote or comment is left for the database to report, except
/// SQLite's unclosed block comment, which it ends at the end of the SQL.
fn trim_raw_statement(sql: &str, backend: DbBackend) -> &str {
    let sql = sql.trim();
    let mut code_end = 0;
    let mut terminator_prefix = None;

    for token in tokens(sql, backend.into()) {
        if !token.terminated && !(backend == DbBackend::SQLite && token.kind == Kind::Comment) {
            return sql;
        }
        let end = token.start + token.text.len();
        match token.kind {
            Kind::Space => {}
            // MySQL runs the body of a `/*! .. */` comment, MariaDB also `/*M! .. */`.
            Kind::Comment
                if !matches!(backend, DbBackend::MySQL | DbBackend::MariaDB)
                    || !(token.text.starts_with("/*!") || token.text.starts_with("/*M!")) => {}
            Kind::Symbol if token.text == ";" => {
                terminator_prefix = Some(code_end);
                code_end = end;
            }
            _ => {
                code_end = end;
                terminator_prefix = None;
            }
        }
    }

    sql[..terminator_prefix.unwrap_or(code_end)].trim_end()
}

#[cfg(test)]
#[path = "../../tests/unit/raw_tests.rs"]
mod tests;
