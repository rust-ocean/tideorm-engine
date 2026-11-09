# tideorm-engine

Internal package that executes TideORM's database work: entities,
active models, query builders, the database drivers, connections and transactions.

TideORM reaches it only through `src/orm.rs`, which re-exports this crate
together with the derive macros from `tideorm-macros` as `tideorm::orm`. The
derives read `#[orm(...)]` attributes and expand to `::tideorm::orm::` paths,
so applications depend on neither crate directly.

## Features

The default feature set is empty, and no feature selects a driver implicitly.

| Feature | Enables |
| --- | --- |
| `postgres` / `mysql` / `sqlite` | The `tideorm-driver` backend for that database |
| `runtime-tokio` / `runtime-async-std` | The driver's async runtime, forwarded only to drivers that are enabled |

## Building

It depends on [tideorm-sql-builder](https://github.com/rust-ocean/tideorm-sql-builder) and
[tideorm-driver](https://github.com/rust-ocean/tideorm-driver) from crates.io. To build against
local checkouts of them, pass a `[patch.crates-io]` config file with `cargo --config <file>`, as
tideorm's CONTRIBUTING.md describes, and keep the `Cargo.lock` changes it makes out of commits.

## Tests

```bash
cargo +1.99.0 test --features sqlite,runtime-tokio
cargo +1.99.0 test --all-features
```

The `live::` contracts also run against a server named by
`ENGINE_PROBE_DATABASE_URL` (PostgreSQL, MySQL or MariaDB):

```bash
cargo +1.99.0 test --no-default-features --features sqlite,postgres,runtime-tokio --tests 'live::'
```
