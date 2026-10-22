//! 栈级配置检查共享引擎（`wist-stackcheck-core`）。
//!
//! **只校验「栈自己生成的东西」**：渲染产物及其新鲜度、目录/挂载约定、密钥文件权限、
//! 站点证书与域名、现场态值。组件配置键的语义由组件自己负责。
//!
//! 每个栈只提供一份 [`profile::Profile`] + 少量栈专属判据；本 crate 提供：
//! - Flow Result v1 的 `Status` / `Finding` / `Stack` / `Options`
//! - 通用判据（渲染、占位值、密钥、身份、挂载、public_url、端口、镜像）
//! - compose / 路径 / TOML / x509 解析 helper
//! - CLI（参数、JSON、文本、结果文件、退出码）
//!
//! 输出对齐 `gops run diagnose` 的 Flow Result v1（`schema_version` + `checks[]`）。

use std::path::PathBuf;

pub mod checks;
pub mod cli;
pub mod profile;
pub mod x509;

/// 单条检查的结论等级。`Ord` 用于取「最差」（`Fail > Warn > Ok`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }

    /// 进程退出码：0 = 全好，1 = 有 WARN，2 = 有 FAIL。
    pub fn exit_code(self) -> u8 {
        match self {
            Status::Ok => 0,
            Status::Warn => 1,
            Status::Fail => 2,
        }
    }
}

/// 一条检查结果。字段名与 Flow Result v1 的 `checks[]` 一致。
#[derive(Debug, Clone)]
pub struct Finding {
    pub code: String,
    pub status: Status,
    /// 它「修不好就别升级 / 别启动」吗？
    pub blocking: bool,
    pub title: String,
    pub detail: String,
    pub hint: String,
}

impl Finding {
    pub fn ok(
        code: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            status: Status::Ok,
            blocking: false,
            title: title.into(),
            detail: detail.into(),
            hint: String::new(),
        }
    }

    pub fn warn(
        code: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status: Status::Warn,
            ..Self::ok(code, title, detail)
        }
    }

    pub fn fail(
        code: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status: Status::Fail,
            blocking: true,
            ..Self::ok(code, title, detail)
        }
    }

    /// 标记「失败但不阻断升级/启动」。
    pub fn non_blocking(mut self) -> Self {
        self.blocking = false;
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
}

/// 被检查的栈根。
#[derive(Debug, Clone)]
pub struct Stack {
    pub root: PathBuf,
}

impl Stack {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 栈内相对路径的绝对化。**不做** `canonicalize` —— 被检查的目标可能还不存在，
    /// 而且宿主要求的是「配置里写的那个路径」，不是它解析后的真身。
    pub fn at(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

/// 运行选项（由 CLI 传入）。
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// 跳过需要联网的检查（镜像可达）。离线站点应该打开。
    pub offline: bool,
    /// 只跑某一组（按 code 前缀过滤，`--only prereq`）。
    ///
    /// 典型用法：`flow localize` **开头**只跑前置条件 —— 那一刻 `apply` 刚换完文件、
    /// 还没重渲，渲染新鲜度会合理地判 FAIL。
    pub only: Option<String>,
}

impl Options {
    pub fn wants(&self, group: &str) -> bool {
        self.only
            .as_deref()
            .is_none_or(|only| group.starts_with(only))
    }
}

/// 最差等级（没有检查项 = `Ok`）。
pub fn worst(findings: &[Finding]) -> Status {
    findings
        .iter()
        .map(|finding| finding.status)
        .max()
        .unwrap_or(Status::Ok)
}

/// 是否存在**阻断类**失败（`--gate` 的判据）。
pub fn has_blocking_failure(findings: &[Finding]) -> bool {
    findings
        .iter()
        .any(|finding| finding.status == Status::Fail && finding.blocking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_of_nothing_is_ok() {
        assert_eq!(worst(&[]), Status::Ok);
        assert!(!has_blocking_failure(&[]));
    }

    #[test]
    fn blocking_flag_separates_gate_failures_from_soft_ones() {
        let soft = Finding::fail("identity.center_files", "t", "").non_blocking();
        let warn = Finding::warn("secret.file_mode", "t", "");
        assert!(!has_blocking_failure(&[soft.clone(), warn.clone()]));
        assert_eq!(worst(&[soft, warn]), Status::Fail);

        let hard = Finding::fail("tls.san_covers_domain", "t", "");
        assert!(has_blocking_failure(&[hard]));
    }

    #[test]
    fn only_filter_matches_by_prefix() {
        let options = Options {
            offline: false,
            only: Some("prereq".to_string()),
        };
        assert!(options.wants("prereq"));
        assert!(!options.wants("render"));
        assert!(!options.wants("package"));
        assert!(Options::default().wants("anything"));
    }
}
