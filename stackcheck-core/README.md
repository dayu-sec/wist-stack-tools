# wist-stackcheck-core

[![crates.io](https://img.shields.io/crates/v/wist-stackcheck-core.svg)](https://crates.io/crates/wist-stackcheck-core)
[![docs.rs](https://docs.rs/wist-stackcheck-core/badge.svg)](https://docs.rs/wist-stackcheck-core)
[![CI](https://github.com/dayu-sec/wist-stack-tools/actions/workflows/ci.yml/badge.svg)](https://github.com/dayu-sec/wist-stack-tools/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/wist-stackcheck-core.svg)](https://github.com/dayu-sec/wist-stack-tools/blob/main/LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://github.com/dayu-sec/wist-stack-tools/blob/main/stackcheck-core/Cargo.toml)

The shared engine behind `wist-stackcheck`: **stack-level configuration checks** for
deployment stacks (Docker Compose + rendered config + site identity).

It turns a stack root into structured **Flow Result v1** findings
(`ok` / `warn` / `fail`, plus `blocking`) with a **read-only** design — no writes,
no `chmod`, no restarts — so it is safe to run before every upgrade.

## Why this crate

Deployment failures are rarely caused by a component being broken. They are usually
caused by **stack-generated configuration that contradicts itself** while every layer
still parses:

- a value file was edited but the rendered config was never regenerated;
- a site variable was overridden, so `localize` writes to one path while Compose mounts another;
- `docker ps` shows a port as occupied by the stack itself;
- `.env` is empty or unreadable and the checker silently falls back to defaults;
- a TLS certificate SAN no longer covers the current domain.

`wist-stackcheck-core` checks exactly those things. It deliberately **does not**
validate component config schemas — components own their key semantics via strict
parsing and their own `diagnose`.

## What it checks

| Group | Codes |
|---|---|
| Variables | `config.vars` |
| Rendering | `render.freshness.*`, `render.placeholder` |
| Domains | `domain.configured` |
| Secrets | `secret.file_mode`, `secret.private_key_mode` |
| Identity / TLS | `identity.*`, `tls.san_covers_domain` |
| Layout | `layout.mounts`, `layout.public_url` |
| Storage | `database.initdb` |
| Runtime surface | `port.available`, `image.reachable` |

All checks are driven by a [`profile::Profile`](https://docs.rs/wist-stackcheck-core/latest/wist_stackcheck_core/profile/struct.Profile.html):
render pairs, secret files, identity files, mount specs, domains and `public_url`
are **data**, not hard-coded paths. A new stack only supplies a profile + its own
stack-specific checks.

## Install

```toml
[dependencies]
wist-stackcheck-core = "0.1"
```

- **MSRV:** Rust 1.88
- **Edition:** 2024
- **License:** Apache-2.0

## Quick start

### 1. Define a profile

```rust
use wist_stackcheck_core::profile::{Profile, PLACEHOLDER_PATTERNS};

static PROFILE: Profile = Profile {
    project_fallback: "my-stack",
    render_pairs: &[],
    placeholder_files: &[],
    placeholder_patterns: PLACEHOLDER_PATTERNS,
    domains: &[],
    secret_files: &[],
    private_key_roots: &["identity"],
    identity_files: &[],
    mounts: &[],
    schema_file: None,
    public_url: None,
    server_cert: None,
};
```

### 2. Compose the checks

```rust
use wist_stackcheck_core::{checks, Finding, Options, Stack};

fn run(stack: &Stack, options: &Options) -> Vec<Finding> {
    let vars = checks::Vars::load(stack);
    let mut out = Vec::new();

    if options.wants("render") {
        out.extend(checks::render_freshness(stack, &PROFILE));
        out.extend(checks::render_placeholders(stack, &PROFILE));
    }
    if options.wants("port") {
        out.extend(checks::ports_available(stack, &vars, &PROFILE));
    }
    out
}
```

### 3. Wrap the CLI

```rust
const USAGE: &str = "my-stackcheck [--root DIR] [--format text|json] [--gate]";

fn main() -> std::process::ExitCode {
    wist_stackcheck_core::cli::run(
        USAGE,
        format!("my-stackcheck {}", env!("CARGO_PKG_VERSION")),
        run,
    )
}
```

`cli::run` provides `--root`, `--format text|json`, `--result`, `--only`, `--gate`
and `--offline`, Flow Result output, result-file append/dedup, and the exit-code
priority described below.

## Output contract

The JSON shape matches [`gops run diagnose`](https://github.com/galaxio-labs/galaxy-ops):

```json
{
  "schema_version": "1",
  "source": "diagnose",
  "checks": [
    {
      "code": "tls.san_covers_domain",
      "status": "fail",
      "blocking": false,
      "title": "leaf certificate SAN does not cover the site domain",
      "detail": "API_DOMAIN=… SAN=[…]",
      "hint": "run `gops sys localize`"
    }
  ]
}
```

| Mode | Exit code |
|---|---|
| `--gate` | `2` if any **blocking** fail; otherwise `0` |
| Result file (`GOPS_DIAGNOSE_RESULT` / `--result`) | always `0`; verdict is in `checks[]` |
| Normal | `0` all ok / `1` warnings / `2` failures |

Priority is **gate > result file > severity**. `--gate` must never be bypassed by an
ambient `GOPS_DIAGNOSE_RESULT`.

## Design principles

- **Stack vs component boundary.** The stack checker validates things the stack
  generates (render freshness, mounts, identity, ports). Components validate their
  own keys.
- **Effective values first.** Read the stack-root `.env`; fall back to
  `sys/merged_vars.yml` only when `.env` does not exist. An empty or unreadable
  `.env` is an error, not a silent fallback.
- **No false green.** “Nothing scanned”, “no source file”, and “cannot determine”
  produce warnings instead of `ok`.
- **Read-only.** No writes, no permission changes, no process restarts. External
  commands are limited to Docker, with timeouts and graceful degradation.
- **Blocking ≠ severity.** A missing signing key is a failure worth fixing, but it
  should not block an otherwise healthy upgrade. Only findings that break *this*
  deployment are blocking.

## Used by

- [`wist-gateway-stack`](https://github.com/dayu-sec/wist-gateway-stack) — `tools/stackcheck`
- [`wist-center-stack`](https://github.com/dayu-sec/wist-center-stack) — `tools/stackcheck`

Design note: [`stack-config-check.md`](https://github.com/dayu-sec/wist-design/blob/main/doc/design/foundation/stack-config-check.md)

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo publish -p wist-stackcheck-core --dry-run
```

Release: push a `vX.Y.Z` tag (or `vX.Y.Z-dryrun` for a dry run). The GitHub Actions
workflow tests, packages and publishes to crates.io. See
[`.github/workflows/release.yml`](https://github.com/dayu-sec/wist-stack-tools/blob/main/.github/workflows/release.yml)
and [CHANGELOG.md](https://github.com/dayu-sec/wist-stack-tools/blob/main/CHANGELOG.md).

## Contributing

Issues and pull requests are welcome. Please keep the read-only guarantee, preserve
the Flow Result v1 contract, and add a regression test for every behavior change.
Security reports: use GitHub Security Advisories.

## License

Apache-2.0. See [LICENSE](https://github.com/dayu-sec/wist-stack-tools/blob/main/LICENSE).
