# syntax=docker/dockerfile:1.7

# ---- Build stage -----------------------------------------------------------
# Built on Debian/glibc (non-Alpine) and cross-compiled to a fully static
# musl binary so the runtime image doesn't need any libc at all.
FROM rust:1-bookworm AS build

RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools ca-certificates binutils \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add x86_64-unknown-linux-musl

# `ring` (pulled in by rustls) has C/asm code; point its build script at the
# musl cross toolchain so everything links statically. RUSTFLAGS forces the
# C runtime to be statically linked too, otherwise the produced binary still
# has a PT_INTERP pointing at /lib/ld-musl-x86_64.so.1 (absent on scratch).
ENV CC_x86_64_unknown_linux_musl=musl-gcc \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
    RUSTFLAGS="-C target-feature=+crt-static"

WORKDIR /src

# Pin per-target rustflags in the project's cargo config so they apply
# regardless of env-var propagation, cache-mount staleness, or dependency
# fingerprinting quirks.
RUN mkdir -p .cargo \
 && printf '[target.x86_64-unknown-linux-musl]\nrustflags = ["-C", "target-feature=+crt-static"]\n' > .cargo/config.toml

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

# Cache only the registry — NOT the target dir — so a changed RUSTFLAGS /
# cargo config forces a genuine rebuild and we never ship stale artifacts.
#
# We pass rustflags via `cargo --config` (highest-priority source) in
# addition to the .cargo/config.toml above, so even if something shadows
# the on-disk config, cargo honours the CLI override.
#
# Verbose output + readelf diagnostics are intentional: if the binary
# ever comes out dynamic again, the CI log will show us exactly which
# source of truth was used and what the ELF actually says.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    echo "--- .cargo/config.toml ---" \
 && cat .cargo/config.toml \
 && echo "--- RUSTFLAGS env ---" \
 && echo "RUSTFLAGS=${RUSTFLAGS}" \
 && echo "--- cargo build ---" \
 && cargo build --release --locked --target x86_64-unknown-linux-musl \
        --config 'target.x86_64-unknown-linux-musl.rustflags=["-C","target-feature=+crt-static"]' \
 && install -D target/x86_64-unknown-linux-musl/release/action-pull-request-merge \
               /out/action-pull-request-merge \
 && echo "--- file(1) ---" \
 && file /out/action-pull-request-merge \
 && echo "--- readelf -d (dynamic deps; should be empty for static) ---" \
 && readelf -d /out/action-pull-request-merge || true \
 && echo "--- assertions ---" \
 && file /out/action-pull-request-merge | grep -qE "(statically linked|static-pie linked)" \
 && { ! file /out/action-pull-request-merge | grep -q "dynamically linked"; } \
 && { ! readelf -d /out/action-pull-request-merge 2>/dev/null | grep -q "NEEDED"; }

# ---- Runtime stage ---------------------------------------------------------
# `scratch` = no OS, no shell. Just the statically-linked binary and the TLS
# root store octocrab's rustls-native-certs looks up at runtime.
FROM scratch

ARG VCS_REF
ARG BUILD_DATE
LABEL org.opencontainers.image.source="https://github.com/sudo-bot/action-pull-request-merge" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.title="action-pull-request-merge" \
      org.opencontainers.image.description="GitHub Action to merge a pull-request"

COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /out/action-pull-request-merge /action-pull-request-merge
ENTRYPOINT ["/action-pull-request-merge"]
