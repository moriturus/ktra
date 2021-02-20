FROM rust:1.50.0 as builder

ARG DB="db-sled"
ARG MIRRORING="crates-io-mirroring"
COPY --chown=rust:rust . /build
WORKDIR /build
RUN cargo build --release --no-default-features --features=secure-auth,${DB},${MIRRORING}

FROM debian:buster-slim

LABEL org.opencontainers.image.source https://github.com/moriturus/ktra
LABEL org.opencontainers.image.documentation https://book.ktra.dev
LABEL org.opencontainers.image.licenses "(Apache-2.0 OR MIT)"

RUN apt update; apt install -y libssl1.1 ca-certificates
COPY --from=builder /build/target/release/ktra ./

ENTRYPOINT [ "./ktra" ]
CMD []