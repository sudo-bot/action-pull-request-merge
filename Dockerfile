# syntax=docker/dockerfile:1.7

# ---- Build stage -----------------------------------------------------------
FROM rust:1.82-alpine AS build
RUN apk add --no-cache musl-dev pkgconfig
WORKDIR /src

# Prime the dependency cache by building a throwaway target first.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && echo '' > src/lib.rs \
 && cargo fetch

# Now copy the real sources and build a statically linked release binary.
COPY src ./src
COPY tests ./tests
RUN cargo build --release --locked \
 && strip target/release/action-pull-request-merge

# ---- Runtime stage ---------------------------------------------------------
FROM alpine:3.20
RUN apk add --no-cache ca-certificates
COPY --from=build /src/target/release/action-pull-request-merge /usr/local/bin/action-pull-request-merge
ENTRYPOINT ["/usr/local/bin/action-pull-request-merge"]
