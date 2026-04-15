# syntax=docker/dockerfile:1.7

# ---- Build stage -----------------------------------------------------------
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src

# Copy the full source tree in one shot. We rely on BuildKit's layer cache
# rather than the fragile "stub crate" trick (which breaks for crates that
# declare both [lib] and [[bin]]).
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
 && cp target/release/action-pull-request-merge /usr/local/bin/action-pull-request-merge \
 && strip /usr/local/bin/action-pull-request-merge

# ---- Runtime stage ---------------------------------------------------------
FROM alpine:3.20
RUN apk add --no-cache ca-certificates
COPY --from=build /usr/local/bin/action-pull-request-merge /usr/local/bin/action-pull-request-merge
ENTRYPOINT ["/usr/local/bin/action-pull-request-merge"]
