# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p qualyx-cli && cp target/release/qualyx /qualyx

# Deliberately small worker toolchain: Python. Extend this image for other ecosystems.
FROM python:3.13-slim-bookworm AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir pytest==8.3.5 \
    && useradd --uid 10001 --create-home qualyx
COPY --from=build /qualyx /usr/local/bin/qualyx
COPY LICENSE-MIT LICENSE-APACHE /usr/share/licenses/qualyx/
USER 10001:10001
WORKDIR /projects
ENTRYPOINT ["qualyx"]
CMD ["--help"]
