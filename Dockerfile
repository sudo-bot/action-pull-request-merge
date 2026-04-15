# syntax=docker/dockerfile:1.7

# ---- Build stage -----------------------------------------------------------
# Built on Debian/glibc (non-Alpine) and cross-compiled to a fully static
# musl binary so the runtime image doesn't need any libc at all.
FROM rust:1-bookworm AS build

RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add x86_64-unknown-linux-musl

# `ring` (pulled in by rustls) has C/asm code; point its build script at the
# musl cross toolchain so everything links statically.
ENV CC_x86_64_unknown_linux_musl=musl-gcc \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

# Use BuildKit cache mounts for the registry and the target dir. The target
# dir is ephemeral when mounted as a cache, so the produced binary is copied
# out to /out/ before the RUN step ends.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --target x86_64-unknown-linux-musl \
 && install -D target/x86_64-unknown-linux-musl/release/action-pull-request-merge \
               /out/action-pull-request-merge

# ---- Runtime stage ---------------------------------------------------------
# `scratch` = no OS, no shell. Just the statically-linked binary and the TLS
# root store octocrab's rustls-native-certs looks up at runtime.
FROM scratch
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /out/action-pull-request-merge /action-pull-request-merge
ENTRYPOINT ["/action-pull-request-merge"]
