FROM rust:latest AS builder

WORKDIR /usr/src/xlsx-merge

COPY Cargo.toml Cargo.lock ./

RUN cargo fetch

COPY src ./src
COPY public ./public

RUN cargo build --release

FROM debian:12-slim

COPY --from=builder /usr/src/xlsx-merge/target/release/xlsx-merge /usr/local/bin/xlsx-merge

WORKDIR /usr/local/bin

CMD ["xlsx-merge"]