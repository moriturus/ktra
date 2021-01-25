FROM ekidd/rust-musl-builder:stable as builder

ARG DB="db-sled"
ARG MIRRORING="crates-io-mirroring"
COPY --chown=rust:rust . /build
WORKDIR /build
RUN cargo build --release --no-default-features --features=secure-auth,${DB},${MIRRORING}

FROM scratch

LABEL org.opencontainers.image.source https://github.com/moriturus/ktra
LABEL org.opencontainers.image.documentation https://book.ktra.dev
LABEL org.opencontainers.image.licenses "(Apache-2.0 OR MIT)"

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/ktra ./

ENTRYPOINT [ "./ktra" ]
CMD []