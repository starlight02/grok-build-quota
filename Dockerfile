# syntax=docker/dockerfile:1.7
#
# 2 阶段：builder（Rust + Node）→ slim runtime
# 层顺序原则：包管理 / 工具链尽量靠前；src 最晚 COPY，避免改业务代码重装依赖。
# 体积：release fat LTO + strip + wasm-opt(z)；速度：mold 链接 + BuildKit cache mount。

FROM rust:bookworm AS builder
WORKDIR /app

# 系统依赖：clang/pkg-config=部分 native crate；mold=链接加速（不增大产物）
# wasm-opt 见下方 binaryen 步（bookworm apt 的 binaryen 过旧）
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        curl \
        mold \
        pkg-config \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

# binaryen(wasm-opt)：bookworm apt 版本(v108)太旧，吃不下新 nightly 产出的 WASM
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

# 编译环境：增量关（Docker 缓存靠 target mount）
# mold 在 COPY .cargo 后以 RUN 追加到 config，不进仓库（避免 GHA 无 mold 失败）
ENV CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0

# cargo-leptos：钉版本，registry cache 可复用；避免 floating latest 反复重装
ARG CARGO_LEPTOS_VERSION=0.3.7
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-leptos --version "${CARGO_LEPTOS_VERSION}" --locked

# ---------- Node deps（不依赖 src，改 .rs 不重装）----------
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./
RUN corepack install \
    && pnpm -v \
    && pnpm install --frozen-lockfile --ignore-scripts

# ---------- Cargo 清单（先于 src，便于 registry 缓存命中）----------
COPY Cargo.toml Cargo.lock leptosfmt.toml rustfmt.toml ./
COPY .cargo ./.cargo
# mold 仅在 Docker builder 启用（GHA quality 未装 mold，勿写进仓库 .cargo/config.toml）
RUN printf '%s\n' \
    '' \
    '[target.x86_64-unknown-linux-gnu]' \
    'linker = "clang"' \
    'rustflags = ["-C", "link-arg=-fuse-ld=mold", "-Zthreads=8"]' \
    '' \
    '[target.aarch64-unknown-linux-gnu]' \
    'linker = "clang"' \
    'rustflags = ["-C", "link-arg=-fuse-ld=mold", "-Zthreads=8"]' \
    >> .cargo/config.toml


# ---------- 源码 + 样式配置（改业务代码只从此层失效）----------
COPY uno.config.ts ./
COPY assets ./assets
COPY src ./src

# release：prebuild 生成 UnoCSS → cargo leptos build --release
# target 缓存跨构建复用；产物抽到 /out 再进 runtime
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    mkdir -p style \
    && pnpm run build \
    && rm -rf node_modules \
    && mkdir -p /out \
    && cp target/release/grok-build-quota /out/ \
    && cp -a target/site /out/site \
    && ls -lh /out/grok-build-quota /out/site/pkg/*.wasm 2>/dev/null || true

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
