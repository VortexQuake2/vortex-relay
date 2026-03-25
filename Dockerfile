# Build stage
FROM rust:1.80-alpine AS builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev

# Pre-build dependencies to cache them
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy source code and build
COPY . .
RUN cargo build --release

# Debug build for remote debugging
# We build both so the user can choose or we can switch easily.
# Alternatively, we could just build debug if requested.
RUN cargo build

# Final stage
FROM alpine:3.20

RUN apk add --no-cache \
    ca-certificates \
    openssl \
    lldb \
    bash \
    python3

WORKDIR /usr/local/bin

COPY --from=builder /usr/src/app/target/release/vrxbridge ./vortex-relay
COPY --from=builder /usr/src/app/target/debug/vrxbridge ./vortex-relay-debug
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Expose the default port and the lldb-server port
EXPOSE 9999 1234

# Use entrypoint script to decide between normal and debug mode
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
