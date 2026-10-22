# wist-stack-tools

跨栈共享工具。当前包含：

- **`stackcheck-core`**：`wist-stackcheck` 的共享引擎（Flow Result v1、CLI、profile 类型、
  compose / 端口 / 镜像 / x509 解析、通用判据）。

`wist-gateway-stack` 与 `wist-center-stack` 各自保留：
- 一份 `StackProfile`（路径 / 变量 / 渲染对 / 身份文件）；
- 少量栈专属判据；
- 薄 `main.rs`。

架构与判据边界见 `wist-design/doc/design/foundation/stack-config-check.md`。

## 版本

以仓库根 `version.txt` 为权威（组件仓，不发布 crates.io）。升级后打 `vX.Y.Z` 标签，
两个栈按标签或 commit rev 固定依赖。
