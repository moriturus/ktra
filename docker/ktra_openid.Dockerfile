FROM rust:1.67-slim-bullseye as builder

ARG DB="db-sled"
ARG MIRRORING="crates-io-mirroring"

RUN apt-get update &&\
    apt-get upgrade -y &&\
    apt-get install -y openssl pkg-config libssl-dev

RUN useradd -m rust
RUN mkdir /build && chown rust:rust /build
USER rust

COPY --chown=rust:rust ./src /build/src
COPY --chown=rust:rust ./Cargo.toml /build/
WORKDIR /build

RUN cargo build --release --no-default-features --features=secure-auth,openid,${DB},${MIRRORING}

FROM debian:bullseye-slim

LABEL org.opencontainers.image.source https://github.com/moriturus/ktra
LABEL org.opencontainers.image.documentation https://book.ktra.dev
LABEL org.opencontainers.image.licenses "(Apache-2.0 OR MIT)"

RUN apt-get update &&\
    apt-get upgrade -y &&\
    apt-get install -y libssl1.1 ca-certificates &&\
    apt-get autoremove -y &&\
    apt-get clean -y

COPY LICENSE-APACHE ./
COPY LICENSE-MIT ./

COPY --from=builder /build/target/release/ktra ./

VOLUME /crates
VOLUME /crates_io_crates
EXPOSE 8000
ENTRYPOINT [ "./ktra" ]
CMD []
