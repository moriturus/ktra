# Ktra 🚚 [![ktra at crates.io](https://img.shields.io/crates/v/ktra.svg)](https://crates.io/crates/ktra)

*Your Little Cargo Registry*.  

`Ktra` is an implementation of the [Alternate Registry](https://doc.rust-lang.org/cargo/reference/registries.html) that is introduced for non-public crates in Rust/Cargo 1.34.

In other words, `Ktra` is an all-in-one package for the private cargo registry.

## Documentations

- [The Ktra Book](https://book.ktra.dev)
    - includes introduction and quick start guide.

## Features

### Current

#### From 0.1.0

- [x] Minimum [Alternate Registry](https://doc.rust-lang.org/cargo/reference/registries.html) implementation.
- [x] [Sled](https://github.com/spacejam/sled) as its internal database.
    - via `db-sled` feature turned on by default.
- [x] Almost pure Rust.

#### From 0.2.0
- [x] Secure user management.

#### From 0.4.0
- [x] [Redis](https://redis.io/) support.
    - via `db-redis` feature.

#### From 0.4.2
- [x] [MongoDB](https://www.mongodb.com/) support.
    - via `db-mongo` feature.

### Planned
- [ ] OAuth and/or OpenID support.
- [ ] RDBMS such as [PostgresQL](https://www.postgresql.org/), [MySQL](https://www.mysql.com/) and [MariaDB](https://mariadb.org/) support.
- [ ] The crates browser like [crate.io](https://crates.io/)

And any feature requests are welcome!

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.