# Changelog

All notable changes to `wist-stackcheck-core` are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.3] - 2026-10-23

### Documentation

- Reworked the README for crates.io / docs.rs: badges, quick start, output
  contract, design principles and contributing guide.

## [0.1.2] - 2026-10-23

### Added

- First crates.io release of the shared engine extracted from
  `wist-gateway-stack` and `wist-center-stack`.
- Flow Result v1 (`Status` / `Finding` / `Stack` / `Options`).
- Profile-driven checks: rendering, placeholders, domains, secrets, identity,
  mounts, `public_url`, database schema, ports and images.
- CLI with `--root`, `--format`, `--result`, `--only`, `--gate`, `--offline`,
  result-file append/de-duplication and exit-code priority.
- Compose / path / TOML / x509 helpers.

## [0.1.0] - 2026-10-23

### Added

- Initial extraction of the shared engine.

[Unreleased]: https://github.com/dayu-sec/wist-stack-tools/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/dayu-sec/wist-stack-tools/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/dayu-sec/wist-stack-tools/releases/tag/v0.1.2
[0.1.0]: https://github.com/dayu-sec/wist-stack-tools/releases/tag/v0.1.0
