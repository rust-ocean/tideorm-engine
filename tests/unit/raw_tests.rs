use super::{limited, next_marker, trim_raw_statement};
use crate::{DbBackend, Statement, Value};

fn postgres(sql: &str) -> &str {
    trim_raw_statement(sql, DbBackend::Postgres)
}

#[test]
fn trims_only_sql_suffixes() {
    for (sql, expected) in [
        ("SELECT 1; -- comment", "SELECT 1"),
        ("SELECT 1 -- comment\n;", "SELECT 1"),
        ("SELECT 1 /* comment */;", "SELECT 1"),
        ("SELECT 1 /* comment */", "SELECT 1"),
        ("SELECT 1; /* one /* two */ one */", "SELECT 1"),
        ("SELECT '--';", "SELECT '--'"),
        ("SELECT \"--;\" -- comment", "SELECT \"--;\""),
        ("SELECT `/*;*/`;", "SELECT `/*;*/`"),
        ("SELECT 'it''s; --';", "SELECT 'it''s; --'"),
        ("SELECT $$--;$$; -- comment", "SELECT $$--;$$"),
        ("SELECT $tag$/*;*/$tag$;", "SELECT $tag$/*;*/$tag$"),
        ("SELECT $1; -- comment", "SELECT $1"),
        ("SELECT 1;; -- comment", "SELECT 1;"),
        (
            "SELECT 1 /* internal */ + 1;",
            "SELECT 1 /* internal */ + 1",
        ),
    ] {
        assert_eq!(postgres(sql), expected, "{sql}");
    }
}

#[test]
fn mysql_comment_and_escape_rules() {
    for (sql, expected) in [
        ("SELECT 1 # comment", "SELECT 1"),
        ("SELECT 1; # comment", "SELECT 1"),
        ("SELECT 1 -- comment", "SELECT 1"),
        ("SELECT 1--2 = 2;", "SELECT 1--2 = 2"),
        ("SELECT 1--2", "SELECT 1--2"),
        (r"SELECT 'a\';' AS x;", r"SELECT 'a\';' AS x"),
        (r"SELECT 1 AS `a\`;", r"SELECT 1 AS `a\`"),
        ("SELECT 1 /*!50000 + 1 */;", "SELECT 1 /*!50000 + 1 */"),
        ("SELECT 1 /*M!100000 + 1 */;", "SELECT 1 /*M!100000 + 1 */"),
    ] {
        assert_eq!(trim_raw_statement(sql, DbBackend::MySQL), expected, "{sql}");
    }
}

#[test]
fn sqlite_quoting_and_comments_follow_sqlite() {
    for (sql, expected) in [
        ("SELECT 1 AS [id--];", "SELECT 1 AS [id--]"),
        (r"SELECT '\' AS id;", r"SELECT '\' AS id"),
        ("SELECT 1 /* outer /* inner */;", "SELECT 1"),
        ("SELECT $tag$ AS id;", "SELECT $tag$ AS id"),
        ("SELECT 1 AS id /* note", "SELECT 1 AS id"),
        ("SELECT 1; /* note", "SELECT 1"),
        ("SELECT '/*' AS id", "SELECT '/*' AS id"),
    ] {
        assert_eq!(
            trim_raw_statement(sql, DbBackend::SQLite),
            expected,
            "{sql}"
        );
    }
}

#[test]
fn postgres_backslashes_are_special_only_in_e_strings() {
    for (sql, expected) in [
        (r"SELECT '\' AS id;", r"SELECT '\' AS id"),
        (r"SELECT E'\\' AS id;", r"SELECT E'\\' AS id"),
    ] {
        assert_eq!(postgres(sql), expected, "{sql}");
    }
}

#[test]
fn leaves_unterminated_constructs_for_the_database() {
    for sql in ["SELECT '--", "SELECT $$--;", "SELECT 1 /*"] {
        assert_eq!(postgres(sql), sql);
    }
}

fn statement(backend: DbBackend, sql: &str, values: usize) -> Statement {
    Statement::from_sql_and_values(
        backend,
        sql,
        (0..values).map(|n| Value::Int(Some(n as i32))),
    )
}

#[test]
fn sqlite_numbers_a_bare_marker_after_the_highest_before_it() {
    // `?2` then a bare `?`, which SQLite numbers 3: the offset goes to `?4`
    // even though only two values are bound, so the missing one still fails.
    let raw = statement(
        DbBackend::SQLite,
        "SELECT * FROM t WHERE a = ?2 AND b = ?",
        2,
    );
    assert_eq!(next_marker(&raw), 4);
    let raw = statement(
        DbBackend::SQLite,
        "SELECT * FROM t WHERE a = ? AND b = ?",
        1,
    );
    assert_eq!(next_marker(&raw), 3);
    // PostgreSQL's `?` is a JSON operator, never a marker.
    let raw = statement(
        DbBackend::Postgres,
        "SELECT * FROM t WHERE doc ? 'k' AND a = $1",
        1,
    );
    assert_eq!(next_marker(&raw), 2);
}

#[test]
fn a_marker_at_the_largest_number_does_not_overflow() {
    let raw = statement(DbBackend::SQLite, "SELECT ?18446744073709551615", 0);
    assert_eq!(next_marker(&raw), usize::MAX);
}

#[test]
fn mysql_bounds_go_before_a_row_lock() {
    for (sql, expected) in [
        (
            "SELECT * FROM t WHERE a = ? FOR UPDATE",
            "SELECT * FROM t WHERE a = ? LIMIT 10 OFFSET ? FOR UPDATE",
        ),
        (
            "SELECT * FROM t FOR SHARE SKIP LOCKED;",
            "SELECT * FROM t LIMIT 10 OFFSET ? FOR SHARE SKIP LOCKED",
        ),
        (
            "SELECT * FROM t LOCK IN SHARE MODE",
            "SELECT * FROM t LIMIT 10 OFFSET ? LOCK IN SHARE MODE",
        ),
        (
            "SELECT * FROM t WHERE a IN (SELECT a FROM u FOR UPDATE)",
            "SELECT * FROM t WHERE a IN (SELECT a FROM u FOR UPDATE) LIMIT 10 OFFSET ?",
        ),
    ] {
        let paged = limited(statement(DbBackend::MySQL, sql, 1), 10, Some(5));
        assert_eq!(paged.sql, expected, "{sql}");
        assert_eq!(paged.values.map(|values| values.0.len()), Some(2));
    }
}
