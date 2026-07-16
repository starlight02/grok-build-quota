# grok-build-quota

Grok Build 额度批量检测工具

技术栈：
- Leptos 0.8 + cargo-leptos（Actix SSR 承载）
- Leptos Server Functions 负责批量检测
- UnoCSS CLI 生成工具类 CSS

## 功能

1. 批量上传 auth JSON（CLIProxyAPI / Grok Build）
2. 服务端探针 `POST https://cli-chat-proxy.grok.com/v1/responses`
3. 结果只显示：账号、状态、额度
4. 一键复制检测结果为 PNG 图片

## 开发

```bash
# 依赖
cargo install cargo-leptos --locked
rustup target add wasm32-unknown-unknown
npm install

# 可选：生成 UnoCSS 产物
npm run css

# 启动
cargo leptos watch
```

打开 http://127.0.0.1:3737

可选代理：

```bash
HTTPS_PROXY=http://127.0.0.1:7890 cargo leptos watch
```

## 代码质量

Leptos 官方推荐工具链（工具链 nightly，组件已在 `rust-toolchain.toml` 声明）：

- `rustfmt`（`rustfmt.toml`）：Rust 代码格式化
- `clippy`（`Cargo.toml [lints]`）：代码质量检查，`all` 组 warn + 禁 `unsafe_code`
- `leptosfmt`（`leptosfmt.toml`）：`view!` 宏 RSX 格式化，`cargo install leptosfmt --locked`

```bash
npm run fmt        # 格式化（先 leptosfmt 后 cargo fmt）
npm run fmt:check  # 只检查不写入
npm run lint       # clippy，-D warnings 作为质量门
npm run check      # fmt:check + lint
```

注意顺序：必须先跑 `leptosfmt` 再跑 `cargo fmt`，反过来两者会在个别链式调用上互相打架。

## 说明

- 浏览器读取 JSON 后，通过 Server Function 发送文件内容到服务端检测
- 响应只返回检测结果，不返回 token
- 图片导出使用 Canvas 绘制结果表，并写入系统剪贴板；失败时回退下载 PNG
