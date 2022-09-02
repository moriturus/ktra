# Ktra 🚚 [![ktra at crates.io](https://img.shields.io/crates/v/ktra.svg)](https://crates.io/crates/ktra)

*Your Little Cargo Registry*.  

`Ktra` is an implementation of the [Alternate Registry](https://doc.rust-lang.org/cargo/reference/registries.html) that is introduced for non-public crates in Rust/Cargo 1.34.

In other words, `Ktra` is an all-in-one package for the private cargo registry.

## Documentations

- [The Ktra Book](https://book.ktra.dev)
    - includes introduction and quick start guide.

## Docker images

```
docker pull ghcr.io/moriturus/ktra:latest
```

All of the docker images are stored at [GitHub Container Registry](https://docs.github.com/en/free-pro-team@latest/packages/getting-started-with-github-container-registry/about-github-container-registry).  
These are public images so you can pull them without any authentication.


Any commit on `develop` branch builds images listed below:

- `latest`
    - `db-sled` featured image.
- `openid-latest`
    - `db-sled` featured image.
    - `openid` support for authentication
- `db-redis-latest`
    - `db-redis` featured image.
- `db-redis-openid-latest`
    - `db-redis` featured image.
    - `openid` support for authentication
- `db-mongo-latest`
    - `db-mongo` featured image.
- `db-mongo-openid-latest`
    - `db-mongo` featured image.
    - `openid` support for authentication


Similarly, images below are built automatically when tags are pushed:

- `{VERSION}` *(e.g. `0.4.3`)*
    - `db-sled` featured image.
- `openid-{VERSION}`
    - `db-sled` featured image.
    - `openid` support for authentication
- `db-redis-{VERSION}`
    - `db-redis` featured image.
- `db-redis-openid-{VERSION}`
    - `db-redis` featured image.
    - `openid` support for authentication
- `db-mongo-{VERSION}`
    - `db-mongo` featured image.
- `db-mongo-openid-{VERSION}`
    - `db-mongo` featured image.
    - `openid` support for authentication

Please see [*"Installation: Docker"* page in The Ktra Book](https://book.ktra.dev/installation/docker.html) for more details.
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

#### From 0.5.0
- [x] [crates.io mirroring](https://github.com/moriturus/ktra/issues/8).
    - via `crates-io-mirroring` feature turned on by default. 

#### From 0.7.0
- [x] OpenID support for auto-discoverable identity providers (e.g. Gitlab, _not_ Github)

### Planned
- [ ] OAuth and/or OpenID support for all identity providers
- [ ] RDBMS such as [PostgresQL](https://www.postgresql.org/), [MySQL](https://www.mysql.com/) and [MariaDB](https://mariadb.org/) support.
- [ ] The crates browser like [crates.io](https://crates.io/)

And any feature requests are welcome!

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
