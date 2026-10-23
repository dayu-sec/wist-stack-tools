# wist-stack-tools

跨栈共享工具。当前包含：

- **`stackcheck-core`**：`wist-stackcheck` 的共享引擎（Flow Result v1、CLI、profile 类型、
  compose / 端口 / 镜像 / x509 解析、通用判据）。

`wist-gateway-stack` 与 `wist-center-stack` 各自保留：
- 一份 `StackProfile`（路径 / 变量 / 渲染对 / 身份文件）；
- 少量栈专属判据；
- 薄 `main.rs`。

架构与判据边界见 `wist-design/doc/design/foundation/stack-config-check.md`。

## 版本与发布（crates.io）

crate：**`wist-stackcheck-core`**（[crates.io](https://crates.io/crates/wist-stackcheck-core) / [docs.rs](https://docs.rs/wist-stackcheck-core)）。

- 版本以仓库根 `version.txt` 为权威；`stackcheck-core/Cargo.toml` 的版本行带
  `#@gxl:set(version)`，用 `gx adm v_patch` / `v_feat` 升版本。
- 发布由 `.github/workflows/publish.yml` 执行：
  - **推荐**：推 `vX.Y.Z` 标签（工作流会校验 tag 与 crate 版本一致），或
  - 手动跑 `workflow_dispatch`（跳过 tag 校验）。
- 仓库 Secret：**`CARGO_REGISTRY_TOKEN`**（crates.io API token，需 `publish-new` /
  `publish-update` 权限）。

> 首次发布：`v0.1.0` 标签早于 publish workflow，推 tag 不会再触发；配好 Secret 后
> 在 Actions 里手动跑一次 **Publish → Run workflow**（`workflow_dispatch`）。之后的新版本走 tag。

```bash
gx adm v_feat                    # 升 version.txt + Cargo.toml
cargo test --workspace
git add -A && git commit -m "release: vX.Y.Z"
git tag vX.Y.Z && git push origin main --tags   # 触发发布
```

发布后，两个栈把依赖从 git tag 切成 registry 版本：

```toml
wist-stackcheck-core = "0.1"
```

然后 `cargo update -p wist-stackcheck-core` 并提交 `Cargo.lock`。
