# wist-stackcheck-core

`wist-stackcheck` 的共享引擎，供 `wist-gateway-stack` 与 `wist-center-stack` 的
`tools/stackcheck` 复用。

## 提供什么

- Flow Result v1 的 `Status` / `Finding` / `Stack` / `Options`
- `StackProfile` 类型：渲染对、密钥文件、身份文件、挂载对、域名、`public_url`
- 通用判据：渲染新鲜度、占位值、密钥权限、身份文件、挂载/落点、`public_url`、
  PG 建表脚本、端口、镜像可达
- compose / 路径 / TOML / x509 helper
- CLI：参数解析、JSON/文本输出、结果文件追加去重、`--gate` 退出码优先级

栈专属判据与 profile 留在各栈仓；本 crate 不含任何栈的路径常量。

## 使用

```toml
[dependencies]
wist-stackcheck-core = "0.1"
```

两个栈当前的参考实现：

- `dayu-sec/wist-gateway-stack` → `tools/stackcheck`
- `dayu-sec/wist-center-stack` → `tools/stackcheck`

设计边界见 `wist-design` 的 `doc/design/foundation/stack-config-check.md`。

## 版本与发布

- 版本以仓库根 `version.txt` 为权威；`Cargo.toml` 的版本行带 `#@gxl:set(version)`。
- 发布由 `.github/workflows/release.yml` 执行：推 `vX.Y.Z` 标签发布；`vX.Y.Z-dryrun`
  只做 `cargo publish --dry-run`。
- Secret：组织级或仓库级 `CRATES_IO_TOKEN`（组织级需授权给本仓）。

## License

Apache-2.0
