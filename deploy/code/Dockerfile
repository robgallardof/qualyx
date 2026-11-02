# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p qualyx-codemode --features sandbox --bin qualyx-code-sandbox \
    && cp target/release/qualyx-code-sandbox /sandbox

FROM debian:bookworm-slim
COPY --from=build /sandbox /sandbox
USER 65532:65532
WORKDIR /
ENTRYPOINT ["/sandbox"]
