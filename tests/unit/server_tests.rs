use super::DbServer;
use crate::DbBackend;

#[test]
fn each_server_renders_its_family_dialect_under_its_own_name() {
    for (server, backend, name) in [
        (DbServer::Postgres, DbBackend::Postgres, "PostgreSQL"),
        (DbServer::CockroachDb, DbBackend::Postgres, "CockroachDB"),
        (DbServer::YugabyteDb, DbBackend::Postgres, "YugabyteDB"),
        (DbServer::MySql, DbBackend::MySQL, "MySQL"),
        (DbServer::TiDb, DbBackend::MySQL, "TiDB"),
        (DbServer::Vitess, DbBackend::MySQL, "Vitess"),
        (DbServer::MariaDb, DbBackend::MariaDB, "MariaDB"),
        (DbServer::Sqlite, DbBackend::SQLite, "SQLite"),
    ] {
        assert_eq!(server.backend(), backend, "{server}");
        assert_eq!(server.to_string(), name);
    }
}
