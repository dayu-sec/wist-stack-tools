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
- 发布流程参考 `wist/wist-contracts`：`.github/workflows/release.yml`
  - 推 `vX.Y.Z` 标签 → 创建 GitHub Release + `cargo publish`；
  - 推 `vX.Y.Z-dryrun` 标签 → 只跑 `cargo publish --dry-run`（不真发）。
- 仓库/组织 Secret：**`CRATES_IO_TOKEN`**（crates.io API token，需 `publish-new` /
  `publish-update` 权限）。
  - 推荐用 **dayu-sec 组织级 Secret**；组织 Secrets 需在 Repository access 里包含
    `wist-stack-tools`（或选 All repositories）。
  - 工作流在创建 Release **之前**检查令牌是否注入；没授权会直接失败，不会留下空 Release。

```bash
gx adm v_feat                    # 升 version.txt + Cargo.toml
cargo test --workspace
git add -A && git commit -m "release: vX.Y.Z"
git tag vX.Y.Z-dryrun && git push origin vX.Y.Z-dryrun   # 可选：先 dry-run
git tag vX.Y.Z && git push origin vX.Y.Z                 # 真发
```

发布后，两个栈把依赖从 git tag 切成 registry 版本：

```toml
wist-stackcheck-core = "0.1"
```

然后 `cargo update -p wist-stackcheck-core` 并提交 `Cargo.lock`。
