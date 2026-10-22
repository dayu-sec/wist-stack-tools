//! CLI 与输出契约（Flow Result v1）。
//!
//! 两个栈只提供：
//! - `usage` 文本与 `--version` 行；
//! - `run(stack, options) -> Vec<Finding>`（按 profile 跑全部判据）。
//!
//! 参数解析、JSON / 文本输出、结果文件追加去重、退出码优先级都在这里，只有一份实现。

use std::path::PathBuf;
use std::process::ExitCode;

use crate::{Finding, Options, Stack, Status, has_blocking_failure, worst};

pub struct Args {
    pub root: PathBuf,
    pub json: bool,
    pub gate: bool,
    pub offline: bool,
    pub only: Option<String>,
    pub result: Option<PathBuf>,
}

impl Args {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut root: Option<PathBuf> = None;
        let mut format = String::from("text");
        let mut gate = false;
        let mut offline = false;
        let mut only: Option<String> = None;
        let mut result: Option<PathBuf> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(None),
                "--gate" => gate = true,
                "--offline" => offline = true,
                "--root" => {
                    let value = args.next().ok_or("--root 需要一个路径")?;
                    root = Some(PathBuf::from(value));
                }
                "--format" => {
                    format = args.next().ok_or("--format 需要 text 或 json")?;
                }
                "--only" => {
                    let value = args.next().ok_or("--only 需要一个 code 前缀")?;
                    if value.trim().is_empty() {
                        return Err("--only 不能为空".to_string());
                    }
                    only = Some(value);
                }
                "--result" => {
                    let value = args.next().ok_or("--result 需要一个路径")?;
                    result = Some(PathBuf::from(value));
                }
                other => return Err(format!("未知参数：{other}")),
            }
        }

        let json = match format.as_str() {
            "text" => false,
            "json" => true,
            other => return Err(format!("--format 只支持 text / json，收到 {other}")),
        };

        // gops 的体检契约：结果写进这个文件，流程本身不该失败。
        if result.is_none()
            && let Ok(path) = std::env::var("GOPS_DIAGNOSE_RESULT")
            && !path.trim().is_empty()
        {
            result = Some(PathBuf::from(path));
        }

        Ok(Some(Self {
            root: root.unwrap_or_else(|| PathBuf::from(".")),
            json,
            gate,
            offline,
            only,
            result,
        }))
    }

    pub fn options(&self) -> Options {
        Options {
            offline: self.offline,
            only: self.only.clone(),
        }
    }
}

/// 跑一次完整的 CLI。
///
/// 退出码优先级：**闸门 > 结果文件 > 严重度**。
pub fn run(
    usage: &str,
    version: String,
    runner: impl FnOnce(&Stack, &Options) -> Vec<Finding>,
) -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // `--version` 不走完整解析：排障时运维只想拿一行版本号。
    if argv.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{version}");
        return ExitCode::SUCCESS;
    }
    let args = match Args::parse(argv.into_iter()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{usage}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{message}\n\n{usage}");
            return ExitCode::from(2);
        }
    };

    let findings = runner(&Stack::new(&args.root), &args.options());
    let verdict = worst(&findings);

    if args.json {
        println!("{}", to_json(&findings));
    } else {
        print_text(&findings, verdict);
    }

    let Some(path) = &args.result else {
        return ExitCode::from(exit_code_for(args.gate, false, &findings));
    };
    if let Err(err) = write_result(path, &to_json(&findings)) {
        eprintln!("写结果文件失败 {}：{err}", path.display());
        return ExitCode::from(2);
    }
    ExitCode::from(exit_code_for(args.gate, true, &findings))
}

/// 退出码优先级：**闸门 > 结果文件 > 严重度**。
///
/// 闸门放第一位是必须的：如果环境里恰好有 `GOPS_DIAGNOSE_RESULT`（或调用方同时传了
/// `--result`），「结果模式始终退 0」会把守门彻底绕过。
pub fn exit_code_for(gate: bool, has_result: bool, findings: &[Finding]) -> u8 {
    if gate {
        return if has_blocking_failure(findings) { 2 } else { 0 };
    }
    if has_result {
        return 0;
    }
    worst(findings).exit_code()
}

/// 写 Flow Result v1；文件已存在时**追加** `checks[]`。
///
/// `gops run diagnose` 只读**一个**结果文件，而一个 flow 里可以有多个诊断动作。
/// 谁都不该默默覆盖前面那份，所以这里做追加：先读旧 `checks`，再接上自己的。
/// 旧文件不是合法 JSON 时告警后重写（不静默丢数据）。
pub fn write_result(path: &std::path::Path, mine: &serde_json::Value) -> std::io::Result<()> {
    // 保留旧文件里的**其他顶层字段**（schema_version / source / 以后新增的），只换 `checks`。
    let mut doc = serde_json::Map::new();
    let mut checks: Vec<serde_json::Value> = Vec::new();

    if let Ok(text) = std::fs::read_to_string(path)
        && !text.trim().is_empty()
    {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(existing) => {
                if let Some(object) = existing.as_object() {
                    if let Some(items) = object.get("checks").and_then(|value| value.as_array()) {
                        checks.extend(items.iter().cloned());
                    }
                    doc = object.clone();
                    doc.remove("checks");
                }
            }
            Err(err) => eprintln!(
                "提示：{} 不是合法的 Flow Result（{err}），将覆盖",
                path.display()
            ),
        }
    }

    if let Some(items) = mine.get("checks").and_then(|value| value.as_array()) {
        // gops 对**同一次运行内重复的 code** 直接拒收整份结果文件，所以这里去重。
        let mut seen: std::collections::HashSet<String> = checks
            .iter()
            .filter_map(|check| check.get("code").and_then(|value| value.as_str()))
            .map(str::to_string)
            .collect();
        for item in items {
            let code = item.get("code").and_then(|value| value.as_str());
            match code {
                Some(code) if !seen.insert(code.to_string()) => {
                    eprintln!("提示：code={code} 已存在，跳过一条重复结果");
                }
                _ => checks.push(item.clone()),
            }
        }
    }

    doc.entry("schema_version".to_string())
        .or_insert_with(|| serde_json::json!("1"));
    doc.entry("source".to_string())
        .or_insert_with(|| serde_json::json!("diagnose"));
    doc.insert("checks".to_string(), serde_json::Value::Array(checks));

    let merged = serde_json::Value::Object(doc);
    std::fs::write(path, format!("{merged}\n"))
}

pub fn to_json(findings: &[Finding]) -> serde_json::Value {
    let checks: Vec<serde_json::Value> = findings
        .iter()
        .map(|finding| {
            serde_json::json!({
                "code": finding.code,
                "status": finding.status.as_str(),
                // `blocking=false`：该修，但不该拦住升级/启动。
                "blocking": finding.blocking,
                "title": finding.title,
                "detail": finding.detail,
                "hint": finding.hint,
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": "1",
        "source": "diagnose",
        "checks": checks,
    })
}

pub fn print_text(findings: &[Finding], verdict: Status) {
    for finding in findings {
        println!(
            "[{:<4}] {} — {}",
            finding.status.as_str().to_uppercase(),
            finding.code,
            finding.title
        );
        if !finding.detail.trim().is_empty() {
            println!("       {}", finding.detail);
        }
        if !finding.hint.trim().is_empty() {
            println!("       → {}", finding.hint);
        }
        if finding.status == Status::Fail && !finding.blocking {
            println!("       （不阻断升级/启动）");
        }
    }

    let total = findings.len();
    let fails = findings.iter().filter(|f| f.status == Status::Fail).count();
    let warns = findings.iter().filter(|f| f.status == Status::Warn).count();
    let oks = total - fails - warns;
    println!();
    println!(
        "{total} 项检查：{oks} ok / {warns} warn / {fails} fail（结论：{}）",
        verdict.as_str()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Args>, String> {
        Args::parse(args.iter().map(|value| value.to_string()))
    }

    #[test]
    fn defaults_to_text_and_current_directory() {
        let args = parse(&[]).unwrap().unwrap();
        assert!(!args.json);
        assert_eq!(args.root, PathBuf::from("."));
        assert!(args.only.is_none());
    }

    #[test]
    fn accepts_root_json_and_only() {
        let args = parse(&["--root", "/tmp/stack", "--format", "json", "--only", "prereq"])
            .unwrap()
            .unwrap();
        assert!(args.json);
        assert_eq!(args.root, PathBuf::from("/tmp/stack"));
        assert_eq!(args.only.as_deref(), Some("prereq"));
    }

    #[test]
    fn rejects_unknown_format() {
        assert!(parse(&["--format", "yaml"]).is_err());
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(parse(&["--nope"]).is_err());
    }

    #[test]
    fn rejects_empty_only() {
        assert!(parse(&["--only", " "]).is_err());
    }

    #[test]
    fn gate_takes_precedence_over_result_mode() {
        let hard = vec![Finding::fail("tls.san_covers_domain", "t", "")];
        let soft = vec![
            Finding::fail("identity.center_files", "t", "").non_blocking(),
            Finding::warn("secret.file_mode", "t", ""),
        ];

        assert_eq!(exit_code_for(true, true, &hard), 2);
        assert_eq!(exit_code_for(true, true, &soft), 0);
        assert_eq!(exit_code_for(false, true, &hard), 0);
        assert_eq!(exit_code_for(false, false, &hard), Status::Fail.exit_code());
    }

    #[test]
    fn to_json_carries_blocking_and_hint() {
        let findings = vec![
            Finding::fail("tls.san_covers_domain", "t", "d").with_hint("h"),
            Finding::fail("identity.center_files", "t", "d").non_blocking(),
        ];
        let doc = to_json(&findings);
        assert_eq!(doc["schema_version"], "1");
        assert_eq!(doc["source"], "diagnose");
        let checks = doc["checks"].as_array().unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0]["blocking"], true);
        assert_eq!(checks[0]["hint"], "h");
        assert_eq!(checks[1]["blocking"], false);
    }

    #[test]
    fn write_result_preserves_other_top_level_fields() {
        let dir = std::env::temp_dir().join(format!(
            "wist-stackcheck-preserve-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("result.json");
        std::fs::write(
            &path,
            r#"{"schema_version":"1","source":"diagnose","extra":{"a":1},"checks":[]}"#,
        )
        .unwrap();

        let mine = serde_json::json!({
            "checks": [{"code": "layout.mounts", "status": "ok", "title": "t"}]
        });
        write_result(&path, &mine).unwrap();

        let merged: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(merged["extra"]["a"], 1);
        assert_eq!(merged["source"], "diagnose");
        assert_eq!(merged["checks"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_result_drops_duplicate_codes() {
        let dir = std::env::temp_dir().join(format!(
            "wist-stackcheck-dedup-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("result.json");
        std::fs::write(
            &path,
            r#"{"schema_version":"1","source":"diagnose","checks":[{"code":"a","status":"ok","title":"t"}]}"#,
        )
        .unwrap();

        let mine = serde_json::json!({
            "checks": [
                {"code": "a", "status": "fail", "title": "dup"},
                {"code": "b", "status": "ok", "title": "new"}
            ]
        });
        write_result(&path, &mine).unwrap();

        let merged: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let codes: Vec<&str> = merged["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|check| check["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, vec!["a", "b"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_result_overwrites_an_unparseable_file() {
        let dir = std::env::temp_dir().join(format!(
            "wist-stackcheck-badjson-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("result.json");
        std::fs::write(&path, "这不是 JSON").unwrap();

        let mine = serde_json::json!({
            "checks": [{"code": "tls.san_covers_domain", "status": "ok", "title": "t"}]
        });
        write_result(&path, &mine).unwrap();

        let merged: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(merged["schema_version"], "1");
        assert_eq!(merged["checks"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
