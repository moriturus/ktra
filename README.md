# Ktra 🚚 [![ktra at crates.io](https://img.shields.io/crates/v/ktra.svg)](https://crates.io/crates/ktra)

*Your Little Cargo Registry*.  

`Ktra` is an implementation of the [Alternate Registry](https://doc.rust-lang.org/cargo/reference/registries.html) that is introduced for non-public crates in Rust/Cargo 1.34.

In other words, `Ktra` is an all-in-one package for the private cargo registry.

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

### Planned
- [ ] OAuth and/or OpenID support.
- [ ] [MongoDB](https://www.mongodb.com/) support.
- [ ] RDBMS such as [PostgresQL](https://www.postgresql.org/), [MySQL](https://www.mysql.com/) and [MariaDB](https://mariadb.org/) support.
- [ ] The crates browser like [crate.io](https://crates.io/)

And any feature requests are welcome!

## Install

```bash
$ cargo install ktra
```

## Quick Start

1. Create the *index git repository*.
    - `Ktra` supports both HTTPS protocol and SSH protocol.
2. Put a file named `config.json` and commit then push it to remote repository.

```bash
$ echo '{"dl":"http://localhost:8000/dl","api":"http://localhost:8000"}' > config.json
$ git add config.json
$ git commit -am "initial commit"
$ git push origin main
```

3. Edit your `.cargo/config.toml`

```toml
[registries]
ktra = { index = "https://github.com/moriturus/ktra-index.git" }
```

4. Create a configuration file.
    - Note 1: `Ktra` searches `./ktra.toml` as a default configuration file if not specified.
    - Note 2: All configurations are able to set via command arguments. See `ktra -h` for more details.

```toml
# essential configurations are `remote_url` and credential informations.
# if you use HTTPS protocol, please specify `https_username` and `https_password` fields.
# using SSH protocol, `ssh_privkey_path` should be specified.
[index_config]
remote_url = "https://github.com/moriturus/ktra-index.git"
https_username = "moriturus"
https_password = "2mdzctfryqirlqbhys43xsc46rbnr93g" 
# ssh_privkey_path = "/path/to/your/private_key"
# name = "committer/author name"
# email = "robot@example.com"

# below configurations are optional.
# [db_config]
# db_path = "my_db" # sled
# redis_url = "redis://username:password@localhost" # redis

# [crate_files_config]
# dl_dir_path = "./crates"
# dl_path = ["download", "path", "to", "mount"]

# [server_config]
# address = "127.0.0.1"
# port = 8080
```

5. Run `ktra`

```bash
$ ktra -c /path/to/config.toml
```

6. Create user and login
    - Note 3: ***From v0.2.0, `ktra` supports and recommends this authentication way.***
    - Note 4: ***⚠️ The authentication way that is used in v0.1.0 is not convertible with the new one. ⚠️***

```bash
$ curl -X POST -H 'Content-Type: application/json' -d '{"password":"PASSWORD"}' http://localhost:8000/ktra/api/v1/new_user/alice
{"token":"0N9mgZb3kzxtgGKECFuMkM2RT5xkYhdY"}
$ cargo login --registry=ktra 0N9mgZb3kzxtgGKECFuMkM2RT5xkYhdY
       Login token for `ktra` saved
```

7. Publish your crate.

```bash
$ cat Cargo.toml
[package]
name = "my_crate"
version = "0.1.0"
authors = ["moriturus <moriturus@alimensir.com>"]
edition = "2018"
description = "sample crate"

[dependencies]
serde = "1.0"

$ cargo publish --registry=ktra
```

8. Use your crate from another crate.

```toml
[package]
name = "my_another_crate"
version = "0.1.0"
authors = ["moriturus <moriturus@alimensir.com>"]
edition = "2018"

[dependencies]
my_crate = { version = "0.1", registry = "ktra" }
```

## Ktra Web APIs

`Ktra Web APIs` are extra web APIs that are not specified in the [specification](https://doc.rust-lang.org/cargo/reference/registries.html) but required to manage users.  
Since all APIs send passwords in ***cleartext***, it is highly recommended that you connect the registry from your local network only *OR* use an HTTPS connection.

### Create a new user

- Specification

<table>
    <tr>
        <td>Endpoint</td>
        <td>/ktra/api/v1/new_user/{user_name}</td>
    </tr>
    <tr>
        <td>Method</td>
        <td>POST</td>
    </tr>
    <tr>
        <td>Body</td>
        <td>{ "password": "PASSWORD" }</td>
    </tr>
</table>

- Response

```json
{
    "token": "TOKEN"
}
```

### Login

- Specification

<table>
    <tr>
        <td>Endpoint</td>
        <td>/ktra/api/v1/login/{user_name}</td>
    </tr>
    <tr>
        <td>Method</td>
        <td>POST</td>
    </tr>
    <tr>
        <td>Body</td>
        <td>{ "password": "PASSWORD" }</td>
    </tr>
</table>

- Response

```json
{
    "token": "NEW TOKEN"
}
```

### Change password

- Specification

<table>
    <tr>
        <td>Endpoint</td>
        <td>/ktra/api/v1/change_password/{user_name}</td>
    </tr>
    <tr>
        <td>Method</td>
        <td>POST</td>
    </tr>
    <tr>
        <td>Body</td>
        <td>{ "old_password": "OLD PASSWORD", "new_password": "NEW PASSWORD" }</td>
    </tr>
</table>

- Response

```json
{
    "token": "NEW TOKEN"
}
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.