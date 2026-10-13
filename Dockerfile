FROM rust:latest AS chef
RUN cargo install cargo-chef
RUN apt-get update && apt-get install -y musl-tools musl-dev
ENV TARGET=x86_64-unknown-linux-musl
RUN rustup target add "$TARGET"
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target "$TARGET" --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --target "$TARGET" \
    && cp target/x86_64-unknown-linux-musl/release/kosync-proxy /app/kosync-proxy

FROM debian:trixie-slim
# The database lands in the working directory by default, so mount a volume here
WORKDIR /data
VOLUME /data
COPY --from=builder /app/kosync-proxy /usr/local/bin/kosync-proxy
EXPOSE 3000 3001
ENTRYPOINT ["kosync-proxy"]
