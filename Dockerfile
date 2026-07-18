# syntax=docker/dockerfile:1.7
#
# 2 阶段：builder（Rust + Node 二进制）→ slim runtime
# pnpm 版本由 package.json#packageManager 决定（corepack install）

FROM rust:bookworm AS builder
WORKDIR /app

# 系统依赖：clang/pkg-config=部分 native crate（wasm-opt 见下方 binaryen 步）
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        curl \
        pkg-config \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

# binaryen(wasm-opt)：bookworm apt 版本(v108)太旧，吃不下新 nightly 产出的 WASM，
# cargo-leptos 末步会报 "wasm-opt optimization failed"；改装官方 release
ARG BINARYEN_VERSION=version_131
RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      amd64) barch=x86_64 ;; \
      arm64) barch=aarch64 ;; \
      *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/WebAssembly/binaryen/releases/download/${BINARYEN_VERSION}/binaryen-${BINARYEN_VERSION}-${barch}-linux.tar.gz" \
      | tar -xz -C /usr/local --strip-components=1; \
    wasm-opt --version

# Node 24 LTS 官方二进制；pnpm 版本稍后按 packageManager 字段安装
ARG NODE_VERSION=24.18.0
RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      amd64) narch=x64 ;; \
      arm64) narch=arm64 ;; \
      *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${narch}.tar.xz" \
      | tar -xJ -C /usr/local --strip-components=1; \
    corepack enable; \
    node -v

# 与仓库 rust-toolchain.toml 对齐（nightly + wasm32）
COPY rust-toolchain.toml ./
RUN rustup show \
    && rustup target add wasm32-unknown-unknown

# cargo-leptos：缓存 registry/git，避免每次全量重装
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-leptos --locked

# pnpm（读 packageManager）→ UnoCSS
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc uno.config.ts ./
COPY src ./src
RUN corepack install \
    && pnpm -v \
    && mkdir -p style \
    && pnpm install --frozen-lockfile --ignore-scripts \
    && pnpm run css \
    && rm -rf node_modules

# Rust 源码 / 清单
COPY Cargo.toml Cargo.lock leptosfmt.toml rustfmt.toml ./
COPY assets ./assets

# release：缓存 target，产物抽到 /out
ENV CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo leptos build --release \
    && mkdir -p /out \
    && cp target/release/grok-build-quota /out/ \
    && cp -a target/site /out/site

# ---------- runtime ----------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /app app

WORKDIR /app
COPY --from=builder /out/grok-build-quota /app/grok-build-quota
COPY --from=builder /out/site /app/site

ENV LEPTOS_OUTPUT_NAME=grok-build-quota \
    LEPTOS_SITE_ROOT=site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:3737 \
    RUST_LOG=info

USER app
EXPOSE 3737
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3737/ >/dev/null || curl -g -fsS http://[::1]:3737/ >/dev/null || exit 1

CMD ["./grok-build-quota"]
