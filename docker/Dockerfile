# syntax=docker/dockerfile:1.7

# ---- Build stage -----------------------------------------------------------
# Build on rust:1-alpine (musl-native) so cargo produces a binary that
# matches the runtime libc without any cross-compilation gymnastics.
FROM rust:1-alpine AS build

# musl-dev provides the C headers / libs that `ring` (pulled in via rustls)
# needs to build its C glue. ca-certificates is not needed at build time;
# it's installed in the runtime stage.
RUN apk add --no-cache musl-dev

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
 && install -D target/release/action-pull-request-merge \
               /out/action-pull-request-merge

# ---- Runtime stage ---------------------------------------------------------
# Alpine 3.23 matches the build image's musl ABI. ca-certificates is
# required because octocrab's rustls-native-certs reads the system trust
# store at runtime for the HTTPS calls to api.github.com.
FROM alpine:3.23

ARG VCS_REF
ARG BUILD_DATE
LABEL org.opencontainers.image.source="https://github.com/sudo-bot/action-pull-request-merge" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.title="action-pull-request-merge" \
      org.opencontainers.image.description="GitHub Action to merge a pull-request"

RUN apk add --no-cache ca-certificates

COPY --from=build /out/action-pull-request-merge /usr/local/bin/action-pull-request-merge
ENTRYPOINT ["/usr/local/bin/action-pull-request-merge"]
