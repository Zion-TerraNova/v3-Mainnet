# ZION OASIS — Rust Backend Server
# Build from V3 workspace root

# ─── Builder ───────────────────────────────────────────────────────────────
FROM rust:1.85-bookworm AS builder

WORKDIR /build

# Cache dependencies
COPY V3/Cargo.toml V3/Cargo.lock ./
COPY V3/L1/cosmic-harmony/Cargo.toml V3/L1/cosmic-harmony/
COPY V3/L1/core/Cargo.toml V3/L1/core/
COPY V3/L1/pool/Cargo.toml V3/L1/pool/
COPY V3/L1/miner/Cargo.toml V3/L1/miner/
COPY V3/L1/native-ffi/Cargo.toml V3/L1/native-ffi/
COPY V3/L2/bridge/Cargo.toml V3/L2/bridge/
COPY V3/L2/dao/Cargo.toml V3/L2/dao/
COPY V3/L2/atomic-swap/Cargo.toml V3/L2/atomic-swap/
COPY V3/L2/swap-aggregator/Cargo.toml V3/L2/swap-aggregator/
COPY V3/L3/ncl/Cargo.toml V3/L3/ncl/
COPY V3/L3/warp/Cargo.toml V3/L3/warp/
COPY V3/L3/ai-native/Cargo.toml V3/L3/ai-native/
COPY V3/L4/oasis/Cargo.toml V3/L4/oasis/
COPY V3/sdk/Cargo.toml V3/sdk/
COPY V3/cli/Cargo.toml V3/cli/

RUN mkdir -p V3/L1/cosmic-harmony/src V3/L1/core/src V3/L1/pool/src \
    V3/L1/miner/src V3/L1/native-ffi/src V3/L2/bridge/src \
    V3/L2/dao/src V3/L2/atomic-swap/src V3/L2/swap-aggregator/src \
    V3/L3/ncl/src V3/L3/warp/src V3/L3/ai-native/src \
    V3/L4/oasis/src V3/sdk/src V3/cli/src && \
    echo 'fn main() {}' > V3/cli/src/main.rs && \
    echo 'fn main() {}' > V3/L1/miner/src/main.rs && \
    echo 'fn main() {}' > V3/L1/pool/src/main.rs

WORKDIR /build/V3
RUN cargo build --release -p zion-oasis 2>/dev/null || true

# Build real source
COPY V3/L1/cosmic-harmony/src V3/L1/cosmic-harmony/src/
COPY V3/L1/core/src V3/L1/core/src/
COPY V3/L1/pool/src V3/L1/pool/src/
COPY V3/L1/miner/src V3/L1/miner/src/
COPY V3/L1/native-ffi/src V3/L1/native-ffi/src/
COPY V3/L2/bridge/src V3/L2/bridge/src/
COPY V3/L2/dao/src V3/L2/dao/src/
COPY V3/L2/atomic-swap/src V3/L2/atomic-swap/src/
COPY V3/L2/swap-aggregator/src V3/L2/swap-aggregator/src/
COPY V3/L3/ncl/src V3/L3/ncl/src/
COPY V3/L3/warp/src V3/L3/warp/src/
COPY V3/L3/ai-native/src V3/L3/ai-native/src/
COPY V3/L4/oasis/src V3/L4/oasis/src/
COPY V3/L4/oasis/data V3/L4/oasis/data/
COPY V3/sdk/src V3/sdk/src/
COPY V3/cli/src V3/cli/src/

RUN cargo build --release -p zion-oasis

# ─── Runtime ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r zion && useradd -r -g zion zion

WORKDIR /app

COPY --from=builder /build/V3/target/release/zion-oasis /usr/local/bin/
COPY --from=builder /build/V3/L4/oasis/data /data/

RUN chown -R zion:zion /app /data

USER zion

ENV OASIS_PORT=8094
ENV OASIS_BIND=0.0.0.0
ENV OASIS_DB=/data/oasis.db
ENV OASIS_AVATARS_PATH=/data/avatars.json
ENV RUST_LOG=info

EXPOSE 8094 8095 9101

ENTRYPOINT ["zion-oasis"]
