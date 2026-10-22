//! 栈级检查（center-stack）。每组只回答一个问题，输出 `Finding`（Flow Result v1 的 `checks[]`）。
//!
//! 判据全部来自**栈自己生成的东西**：`sys/merged_vars.yml` / `.env`（变量）、`configs/*`
//! （渲染产物）、`sys/docker-compose.yml`（挂载）、`identity/center/`（站点身份）、`.wist/`（密钥）。
//!
//! 每个 `code` 在一次运行里**只出现一次**（gops 对重复 code 会整份拒收结果文件）。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::profile::{MountKind, Profile};
use crate::{Finding, Stack};

// ── 变量：生效值，不是默认值 ───────────────────────────────────────────────

/// 生效变量。`error` 有值时所有依赖变量的检查都返回空 —— 由 `config.vars` 一条报出，
/// 不让同一件事刷出十几条 FAIL。
pub struct Vars {
    pub map: BTreeMap<String, String>,
    /// `.env` 或 `sys/merged_vars.yml（默认值）`。
    pub source: &'static str,
    pub error: Option<String>,
}

impl Vars {
    pub fn load(stack: &Stack) -> Self {
        match effective_vars(stack) {
            Ok((map, source)) => Self {
                map,
                source,
                error: None,
            },
            Err(err) => Self {
                map: BTreeMap::new(),
                source: "不可用",
                error: Some(err),
            },
        }
    }

    pub fn ok(&self) -> bool {
        self.error.is_none()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

/// 读 `sys/merged_vars.yml` 的 `- name: / value:` 对。
///
/// 用行扫描而不是引入 YAML 依赖：该文件由 gops 生成、形状固定（`- name: X` 紧接
/// `  value: Y`）。读不到时返回 `Err` —— 让检查报 FAIL，而不是静默当成「没配」。
pub fn merged_vars(stack: &Stack) -> Result<BTreeMap<String, String>, String> {
    let path = stack.at("sys/merged_vars.yml");
    let text =
        fs::read_to_string(&path).map_err(|err| format!("读不到 {}：{err}", path.display()))?;
    let mut out = BTreeMap::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- name:") {
            name = Some(unquote(rest.trim()).to_string());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("value:")
            && let Some(current) = name.take()
        {
            out.insert(current, unquote(rest.trim()).to_string());
        }
    }
    // 文件在但一条也没解出来 = 形状变了（改名 / 截断 / 换成别的 YAML 风格）。
    // 不能静默返回空表：下面的检查会回落到缺省值，于是「什么都没配」看起来像「配对了」。
    if out.is_empty() {
        return Err(format!(
            "{} 里没有任何 `- name:` / `value:` 条目（格式不认识）",
            path.display()
        ));
    }
    Ok(out)
}

/// 读**生效值**：`gops sys localize` 把合并后的值写在栈根 `.env`。
///
/// 拿 `sys/merged_vars.yml`（INITIAL / 默认值）去判会错：现场把 `CENTER_CONFIG_DIR` 覆盖成
/// 别的路径（完全合法的现场选择）时，路径自洽会永远对不上，而这类项一旦误判就变成
/// 「升级被闸门拦住」。
pub fn effective_vars(stack: &Stack) -> Result<(BTreeMap<String, String>, &'static str), String> {
    let env = stack.at(".env");
    match fs::read_to_string(&env) {
        Ok(text) => {
            let mut out = BTreeMap::new();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    out.insert(key.trim().to_string(), unquote(value.trim()).to_string());
                }
            }
            // 文件在但一条也没解出来：不能当「不存在」去回落默认值 ——
            // 现场覆盖全丢时，拿默认值判会得出一份假绿。
            if out.is_empty() {
                return Err(format!(
                    "{} 里没有任何 KEY=VALUE 条目（空文件或格式不认识）",
                    env.display()
                ));
            }
            return Ok((out, ".env"));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        // `.env` 存在但读不了（权限 / 是个目录）：**不能**静默回落默认值 ——
        // 那正是「检查器读默认值、现场用生效值」导致误判/漏判的那条老路。
        Err(err) => return Err(format!("读不到 {}：{err}", env.display())),
    }
    // 还没 localize 过（首次部署）：只能拿默认值，并在 detail 里写明来源。
    Ok((merged_vars(stack)?, "sys/merged_vars.yml（默认值）"))
}

pub fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

pub fn env_var(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(unquote(value.trim()).to_string());
        }
    }
    None
}

// ── 通用工具 ────────────────────────────────────────────────────────────────

/// 在 TOML 文档里定位某个键，返回它**所在段的路径**（文档根 = 空串）。
pub fn locate_key(value: &toml::Value, key: &str, prefix: &str) -> Option<String> {
    match value {
        toml::Value::Table(table) => {
            if table.contains_key(key) {
                return Some(prefix.to_string());
            }
            for (name, child) in table {
                let child_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                if let Some(hit) = locate_key(child, key, &child_prefix) {
                    return Some(hit);
                }
            }
            None
        }
        toml::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                if let Some(hit) = locate_key(child, key, &format!("{prefix}[{index}]")) {
                    return Some(hit);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
}

pub fn trim_slash(value: &str) -> &str {
    value.trim_end_matches('/')
}

/// 词法归一：去掉 `.` 分量并折叠 `..`。
///
/// 不能用 `canonicalize`：被检查的路径可能还不存在（现场首次部署），而且它会把符号链接
/// 解析成真身 —— 而这里要比的是「配置里写的那个路径」。
pub fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 展开 compose 里的 `${VAR}` / `${VAR:-default}`（只认这两种）。
pub fn expand(raw: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let expr = &after[..end];
        let (name, default) = match expr.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (expr, None),
        };
        match vars.get(name.trim()) {
            Some(value) if !value.trim().is_empty() => out.push_str(value),
            _ => out.push_str(default.unwrap_or("")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn resolve_host_path(stack: &Stack, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&stack.root.join(path))
    }
}

pub fn display_rel(stack: &Stack, path: &Path) -> String {
    path.strip_prefix(&stack.root)
        .unwrap_or(path)
        .display()
        .to_string()
}

pub fn compose_file(stack: &Stack) -> Option<PathBuf> {
    // 与 gops 的 compose 发现同口径（`sys/docker-compose.yml` 优先，也认仓根旧布局）。
    const CANDIDATES: &[&str] = &[
        "sys/docker-compose.yml",
        "sys/docker-compose.yaml",
        "sys/compose.yml",
        "sys/compose.yaml",
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ];
    CANDIDATES
        .iter()
        .map(|name| stack.at(name))
        .find(|path| path.is_file())
}

pub fn is_placeholder(value: &str) -> bool {
    let lowered = value.trim().trim_end_matches('.').to_ascii_lowercase();
    // 按**域名边界**判，不用裸子串：`myexample.com` 不是占位值。
    lowered == "example.com"
        || lowered.ends_with(".example.com")
        || lowered == "warpinsight.example"
        || lowered.ends_with(".warpinsight.example")
        || lowered.contains("changeme")
}

pub fn domain_value(vars: &Vars, spec: &crate::profile::DomainSpec) -> Option<String> {
    if let Some(raw) = vars.get(spec.var) {
        return Some(raw.to_string());
    }
    let derive = spec.derive.as_ref()?;
    let from = vars.get(derive.from)?;
    Some(derive.pattern.replace("{}", from))
}

// ── 1. 渲染产物新鲜度 ──────────────────────────────────────────────────────

/// 渲染产物（`gx.tpl` 的输出）必须不早于它的值文件 —— 否则就是「改了值没重渲」。
pub fn render_freshness(stack: &Stack, profile: &Profile) -> Vec<Finding> {
    let mut out = Vec::new();
    for pair in profile.render_pairs {
        let rendered = stack.at(pair.rendered);
        let source = stack.at(pair.source);
        let (rendered_at, source_at) = (modified(&rendered), modified(&source));
        match (rendered_at, source_at) {
            // 两侧都在：正常判。
            (Some(rendered_at), Some(source_at)) => {
                if rendered_at < source_at {
                    out.push(
                        Finding::fail(
                            pair.code,
                            "渲染产物比它的值文件旧",
                            format!("{} 早于 {}", pair.rendered, pair.source),
                        )
                        .with_hint("跑一次 `gops sys localize`（改了值必须重渲才生效）"),
                    );
                } else {
                    out.push(Finding::ok(
                        pair.code,
                        "渲染产物不早于值文件",
                        pair.rendered.to_string(),
                    ));
                }
            }
            // 只有值文件：渲染没生成 → 该跑 localize（不能当成「这个组未部署」而静默跳过）。
            (None, Some(_)) => out.push(
                Finding::warn(
                    pair.code,
                    "值文件在，但渲染产物缺失",
                    pair.rendered.to_string(),
                )
                .with_hint("跑一次 `gops sys localize`"),
            ),
            // 只有渲染产物：来源（值文件 / 模板）不见了，判不了新旧，但也不能报绿。
            (Some(_), None) => out.push(
                Finding::warn(
                    pair.code,
                    "有渲染产物但找不到它的值文件/模板",
                    format!("{}；无法判定是否过期", pair.rendered),
                )
                .with_hint("确认包里没少文件；再跑一次 `gops sys localize`"),
            ),
            // 两侧都没有：这个组未部署，静默跳过。
            (None, None) => {}
        }
    }
    out
}

// ── 2. 占位符 ──────────────────────────────────────────────────────────────

/// 渲染产物里不该留占位值（`example.com` / `warpinsight.example` 之类）——
/// 否则脚本会拿它当真值去连，而且往往没人发现。
pub fn render_placeholders(stack: &Stack, profile: &Profile) -> Vec<Finding> {
    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for rel in profile.placeholder_files {
        let Ok(text) = fs::read_to_string(stack.at(rel)) else {
            continue;
        };
        scanned += 1;
        for (index, line) in text.lines().enumerate() {
            let lowered = line.to_ascii_lowercase();
            for pattern in profile.placeholder_patterns {
                if lowered.contains(&pattern.to_ascii_lowercase()) {
                    hits.push(format!("{rel}:{}: {pattern}", index + 1));
                }
            }
        }
    }

    if hits.is_empty() {
        // 一个文件都没扫到 = `--root` 指错了，或配置根本还没渲染。不能报绿：
        // 那会让「什么都没查」看起来像「都没问题」。
        if scanned == 0 {
            return vec![Finding::warn(
                "render.placeholder",
                "找不到任何渲染产物，跳过占位值检查",
                "configs/ 下没有可读的配置文件（--root 指对了吗？跑过 localize 吗？）",
            )];
        }
        return vec![Finding::ok(
            "render.placeholder",
            "渲染产物里没有占位值",
            format!("扫描 {scanned} 个文件"),
        )];
    }
    let shown = hits.iter().take(10).cloned().collect::<Vec<_>>().join("；");
    vec![
        Finding::fail(
            "render.placeholder",
            "渲染产物里还有占位值",
            format!("{} 处：{shown}", hits.len()),
        )
        .with_hint("把站点域名写进 values/ 后重渲；占位值会让客户端连到不存在的域名"),
    ]
}

// ── 3. 域名 ────────────────────────────────────────────────────────────────

/// 每个域名都必须由现场给出（或按 profile 的派生规则得到），不能停在默认占位值。
pub fn domain_configured(vars: &Vars, profile: &Profile) -> Vec<Finding> {
    const CODE: &str = "domain.configured";
    if !vars.ok() || profile.domains.is_empty() {
        return Vec::new();
    }

    let mut problems = Vec::new();
    let mut values = Vec::new();
    for spec in profile.domains {
        let value = domain_value(vars, spec);
        match value.as_deref() {
            None => problems.push(format!("{} 为空（且无法派生）", spec.var)),
            Some(value) if is_placeholder(value) => {
                problems.push(format!("{} 还是占位值 `{value}`", spec.var));
            }
            Some(_) => {}
        }
        values.push(format!("{}={}", spec.var, value.unwrap_or_default()));
    }

    if problems.is_empty() {
        vec![Finding::ok(
            CODE,
            "站点域名都已配置",
            format!("{}（变量来源 {}）", values.join("，"), vars.source),
        )]
    } else {
        vec![
            Finding::fail(
                CODE,
                "站点域名还是默认值",
                format!("{}（变量来源 {}）", problems.join("；"), vars.source),
            )
            .with_hint(
                "在 ops-prj 的 values/<系统名>/sys_value.yml 里写站点域名，再 `gops sys localize`",
            ),
        ]
    }
}

// ── 4. 密钥文件权限 ────────────────────────────────────────────────────────

/// 栈根密钥与含凭据的渲染物不能被「其他用户」读到。权限位语义各家不同，这里只看 world 位。
#[cfg(not(unix))]
pub fn secret_perms(_stack: &Stack, _profile: &Profile) -> Vec<Finding> {
    vec![Finding::ok(
        "secret.file_mode",
        "非 unix 平台，跳过权限检查",
        "",
    )]
}

#[cfg(unix)]
pub fn secret_perms(stack: &Stack, profile: &Profile) -> Vec<Finding> {
    use crate::profile::Missing;
    use std::os::unix::fs::PermissionsExt;

    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let mut blocking = false;
    let mut checked = 0usize;

    for spec in profile.secret_files {
        let path = stack.at(spec.path);
        match fs::metadata(&path) {
            Ok(meta) => {
                checked += 1;
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o007 != 0 {
                    problems.push(format!(
                        "{}（{}）权限 {:o}：其他用户可读",
                        spec.path, spec.label, mode
                    ));
                    blocking |= spec.blocking;
                } else if mode & 0o020 != 0 {
                    // 属组可写 = 同组任何人都能改写口令牌。不能一律阻断（有些现场靠 660
                    // 让容器属组写入），但必须提示。
                    warnings.push(format!("{} 权限 {:o}：属组可写", spec.path, mode));
                }
            }
            Err(_) => match spec.missing {
                Missing::Skip => {}
                Missing::Warn => warnings.push(format!("缺少 {}（{}）", spec.path, spec.label)),
                Missing::Fail => {
                    problems.push(format!("缺少 {}（{}）", spec.path, spec.label));
                    blocking |= spec.blocking;
                }
            },
        }
    }

    let mut detail = problems.join("；");
    if !warnings.is_empty() {
        if !detail.is_empty() {
            detail.push_str("；另：");
        }
        detail.push_str(&warnings.join("；"));
    }

    if !problems.is_empty() {
        let mut finding = Finding::fail("secret.file_mode", "密钥 / 含凭据文件权限不合格", detail)
            .with_hint("栈根密钥与含口令的渲染物应为 600/640；world 位一律收回");
        if !blocking {
            finding = finding.non_blocking();
        }
        return vec![finding];
    }
    if !warnings.is_empty() {
        return vec![
            Finding::warn("secret.file_mode", "密钥 / 含凭据文件权限需注意", detail)
                .with_hint("若不需要属组写入，收到 640"),
        ];
    }
    vec![Finding::ok(
        "secret.file_mode",
        "密钥与凭据文件权限正常",
        format!("检查 {checked} 个文件"),
    )]
}

/// 私钥：扫 `identity/`，按**内容**识别（不靠文件名猜），world 可读一律 FAIL。
pub fn private_key_mode(stack: &Stack, profile: &Profile) -> Vec<Finding> {
    #[cfg(not(unix))]
    {
        let _ = (stack, profile);
        return vec![Finding::ok(
            "secret.private_key_mode",
            "非 unix 平台，跳过权限检查",
            "",
        )];
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut files = Vec::new();
        for root in profile.private_key_roots {
            walk_files(&stack.at(root), 5, &mut files);
        }

        let mut leaked = Vec::new();
        let mut checked = 0usize;
        for path in files {
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.len() > 512 * 1024 {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains("PRIVATE KEY") {
                continue;
            }
            checked += 1;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o007 != 0 {
                leaked.push(format!("{}（{:o}）", display_rel(stack, &path), mode));
            }
        }

        if !leaked.is_empty() {
            return vec![
                Finding::fail(
                    "secret.private_key_mode",
                    "私钥对其他用户可读",
                    leaked.join("；"),
                )
                .with_hint("私钥应为 600/640；world 位一律收回"),
            ];
        }
        if checked == 0 {
            // 没扫到 ≠ 没问题：`identity/` 可能还没生成，或私钥被挪走了。
            return vec![Finding::warn(
                "secret.private_key_mode",
                "没有扫到任何私钥文件",
                format!(
                    "扫描 {} 个根目录；站点身份尚未生成？",
                    profile.private_key_roots.join("、")
                ),
            )];
        }
        vec![Finding::ok(
            "secret.private_key_mode",
            "私钥文件不对其他用户开放",
            format!("检查 {checked} 个私钥文件"),
        )]
    }
}

pub fn walk_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, depth - 1, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

// ── 5. 站点身份文件 ────────────────────────────────────────────────────────

/// 身份是「证书 + 私钥」一对：缺叶证书/私钥会直接坏；缺 CA 私钥或 CA-G 只影响以后的操作。
pub fn identity_files(stack: &Stack, profile: &Profile) -> Vec<Finding> {
    const CODE: &str = "identity.center_files";
    let mut missing = Vec::new();
    let mut blocking = false;
    let mut checked = 0usize;

    for spec in profile.identity_files {
        checked += 1;
        if stack.at(spec.path).is_file() {
            continue;
        }
        missing.push(format!("{}（{}）", spec.path, spec.label));
        blocking |= spec.blocking;
    }

    if missing.is_empty() {
        return vec![Finding::ok(
            CODE,
            "站点身份文件齐全",
            format!("检查 {checked} 项"),
        )];
    }
    let mut finding = Finding::fail(CODE, "站点身份文件缺失", missing.join("；")).with_hint(
        "跑一次 `gops sys localize`（init-center.sh 会补齐身份）；\
         CA 私钥与 CA-G 丢了要显式轮换，不能静默重生成",
    );
    if !blocking {
        finding = finding.non_blocking();
    }
    vec![finding]
}

// ── 6. TLS 证书 ────────────────────────────────────────────────────────────

/// 中心叶证书必须在有效期内、且 SAN 覆盖**两个**域名（管理面 + 网关面）。
///
/// 不能用子串匹配：`*.a.b` 只覆盖**一层**，`gw.example.com.evil.net` 也不能算覆盖
/// `gw.example.com`。
pub fn layout_mounts(stack: &Stack, vars: &Vars, profile: &Profile) -> Vec<Finding> {
    const CODE: &str = "layout.mounts";

    let Some(compose_path) = compose_file(stack) else {
        return vec![
            Finding::fail(
                CODE,
                "找不到 compose 文件",
                "按 gops 的候选顺序（sys/docker-compose.yml 等）都没找到",
            )
            .with_hint("确认栈目录完整（`sys/docker-compose.yml` 随制品发布）"),
        ];
    };
    let compose = match fs::read_to_string(&compose_path) {
        Ok(text) => text,
        Err(err) => {
            return vec![Finding::fail(
                CODE,
                "读不到 compose 文件",
                format!("{}：{err}", compose_path.display()),
            )];
        }
    };
    if !vars.ok() {
        return Vec::new();
    }

    let mut problems = Vec::new();
    for spec in profile.mounts {
        match compose_mount_target(&compose, spec.source_expr) {
            None => {
                problems.push(format!(
                    "compose 里找不到 `{}:{}` 的挂载",
                    spec.source_expr, spec.target
                ));
                continue;
            }
            Some(found) if trim_slash(&found) != trim_slash(spec.target) => {
                problems.push(format!(
                    "`{}` 挂到 `{found}`，配置期望 `{}`",
                    spec.source_expr, spec.target
                ));
                continue;
            }
            Some(_) => {}
        }

        let expanded = expand(spec.source_expr, &vars.map);
        let actual = resolve_host_path(stack, &expanded);
        let expected = resolve_host_path(stack, spec.expected_host);
        if actual != expected {
            problems.push(format!(
                "`{}` 展开为 `{}`，但 localize 写到 `{}`（变量被覆盖成别的路径？）",
                spec.source_expr,
                display_rel(stack, &actual),
                spec.expected_host
            ));
        }

        let exists = match spec.kind {
            MountKind::File => expected.is_file(),
            MountKind::Dir => expected.is_dir(),
        };
        if !exists {
            problems.push(format!(
                "`{}` 不存在（{}）",
                spec.expected_host,
                match spec.kind {
                    MountKind::File => "文件",
                    MountKind::Dir => "目录",
                }
            ));
        }
    }

    if problems.is_empty() {
        vec![Finding::ok(
            CODE,
            "compose 挂载与落点自洽",
            format!(
                "检查 {} 条挂载（变量来源 {}）",
                profile.mounts.len(),
                vars.source
            ),
        )]
    } else {
        vec![
            Finding::fail(CODE, "compose 挂载与落点不一致", problems.join("；")).with_hint(
                "别单独覆盖 CENTER_CONFIG_DIR / CENTER_ARTIFACT_DIR / GIT_PROXY_CONFIG_DIR / \
                 PG_INITDB_DIR / WEB_NGINX_CONF：localize 按固定路径渲染；\
                 缺目录先跑 `gops sys localize`（Docker 会把缺失目录建成 root:root）",
            ),
        ]
    }
}

/// 从 compose 文本里取某个 `${VAR}` 源对应的容器 target。
/// 短语法（`- src:dst[:ro]`）与长语法（`source:` + `target:`）都认。
pub fn compose_mount_target(compose: &str, source_expr: &str) -> Option<String> {
    let lines: Vec<&str> = compose.lines().collect();

    // 短语法：`- ${PACKAGE_DIR}:/packages:ro`
    for line in &lines {
        let Some(rest) = line.trim().strip_prefix("- ") else {
            continue;
        };
        let rest = rest.trim_matches('"').trim_matches('\'');
        let Some((src, dst)) = rest.split_once(':') else {
            continue;
        };
        if src.trim() != source_expr {
            continue;
        }
        return Some(
            dst.split(':')
                .next()
                .unwrap_or(dst)
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        );
    }

    // 长语法：
    //   - type: bind
    //     source: ${PACKAGE_DIR}
    //     target: /packages
    // `target` 可能在本条目的 source 之前或之后；只在**同一条目**里找
    // （遇到下一条 `- ` 就停），否则会把上一个挂载的 target 配给下一个 source。
    for (index, line) in lines.iter().enumerate() {
        let Some((_, value)) = line.split_once("source:") else {
            continue;
        };
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches(',');
        if value != source_expr {
            continue;
        }
        for candidate in lines.iter().skip(index + 1) {
            if candidate.trim().starts_with("- ") {
                break;
            }
            if let Some((_, target)) = candidate.split_once("target:") {
                return Some(clean_mount_value(target));
            }
        }
        for candidate in lines[..index].iter().rev() {
            if candidate.trim().starts_with("- ") {
                // 本条目的起始行：它自己也可能带 target（`- target: /packages`）。
                if let Some((_, target)) = candidate.split_once("target:") {
                    return Some(clean_mount_value(target));
                }
                break;
            }
            if let Some((_, target)) = candidate.split_once("target:") {
                return Some(clean_mount_value(target));
            }
        }
    }
    None
}

pub fn clean_mount_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(',')
        .to_string()
}

// ── 8. 现场态 public_url ──────────────────────────────────────────────────

/// `public_url` 是站点现场事实：声明只写 `${VAR}`，真值在旁边的 `.env`。
///
/// 两个必须查的矛盾：
/// 1. 主配置里**不能**再出现 `public_url`（两份配置叶子键重复会让组件拒启）；
/// 2. `.env` 里的值必须与当前域名 / 端口一致 —— 改了域名但 `.env` 不跟着改
///    （localize 永不覆盖现场值），客户端会继续连旧地址，且长期没人发现。
pub fn public_url(stack: &Stack, vars: &Vars, profile: &Profile) -> Vec<Finding> {
    const CODE: &str = "layout.public_url";
    let Some(spec) = profile.public_url.as_ref() else {
        return Vec::new();
    };
    let mut blocking_problems = Vec::new();
    let mut soft_problems = Vec::new();
    let mut warnings = Vec::new();

    // 1) 主配置不得重复定义 public_url。
    let main = stack.at(spec.main_config);
    match fs::read_to_string(&main) {
        Ok(text) => match toml::from_str::<toml::Value>(&text) {
            Ok(doc) => {
                if let Some(section) = locate_key(&doc, "public_url", "") {
                    let where_ = if section.is_empty() {
                        "文档根".to_string()
                    } else {
                        format!("段 `{section}`")
                    };
                    blocking_problems.push(format!(
                        "主配置 {} 的{where_}里出现了 public_url；两份配置叶子键重复会让中心拒启",
                        spec.main_config
                    ));
                }
            }
            Err(err) => {
                blocking_problems.push(format!("主配置 {} 无法解析：{err}", spec.main_config))
            }
        },
        Err(_) => warnings.push(format!(
            "主配置 {} 尚未渲染（跳过重复键检查）",
            spec.main_config
        )),
    }

    // 2) 现场态声明必须在。
    let declaration = stack.at(spec.declaration);
    if !declaration.is_file() {
        blocking_problems.push(format!(
            "缺少现场态声明 {}（组件读取的现场态配置）",
            spec.declaration
        ));
    } else if let Ok(text) = fs::read_to_string(&declaration)
        && let Ok(doc) = toml::from_str::<toml::Value>(&text)
    {
        let value = doc
            .get("server")
            .and_then(|server| server.get("public_url"))
            .and_then(|value| value.as_str());
        match value {
            None => {
                blocking_problems.push(format!("{} 里没有 `[server] public_url`", spec.declaration))
            }
            Some(raw) if !raw.contains(&format!("${{{}", spec.env_var)) => warnings.push(format!(
                "{} 的 public_url 不是 `${{{}}}` 引用（值被写死，改域名不会生效）",
                spec.declaration, spec.env_var
            )),
            Some(_) => {}
        }
    }

    // 3) `.env` 的生效值必须与当前域名 / 端口一致。
    if vars.ok() {
        match env_var(&stack.at(spec.env), spec.env_var) {
            None => warnings.push(format!(
                "{} 没有 {}；组件会回落到声明里的默认值（占位域名）",
                spec.env, spec.env_var
            )),
            Some(value) if value.trim().is_empty() => warnings.push(format!(
                "{} 的 {} 为空；组件会回落到声明里的默认值（占位域名）",
                spec.env, spec.env_var
            )),
            Some(value) => {
                if let Some(expected) = expected_public_url(vars, profile, spec)
                    && normalize_url(&value) != normalize_url(&expected)
                {
                    soft_problems.push(format!(
                        "{}={}，但按当前变量应为 {}（改域名后 .env 不会自动更新）",
                        spec.env_var, value, expected
                    ));
                }
            }
        }
    }

    if !blocking_problems.is_empty() {
        return vec![
            Finding::fail(
                CODE,
                "现场态 public_url 与主配置冲突",
                blocking_problems.join("；"),
            )
            .with_hint("主配置里不能出现 public_url（叶子键与现场态声明重复会拒启）"),
        ];
    }
    if !soft_problems.is_empty() {
        // 不阻断：跑着的栈还是好的，错的只是「以后生成的网关安装 URL」。
        return vec![
            Finding::fail(
                CODE,
                "现场态 public_url 与当前域名/端口不一致",
                soft_problems.join("；"),
            )
            .non_blocking()
            .with_hint(
                format!(
                    "改 {} 的 {}（localize 永不覆盖现场值）；改域名后必须手动跟着改，\
                     否则客户端会连到旧地址",
                    spec.env, spec.env_var
                ),
            ),
        ];
    }
    if !warnings.is_empty() {
        return vec![Finding::warn(
            CODE,
            "现场态 public_url 尚未定型",
            warnings.join("；"),
        )];
    }
    vec![Finding::ok(
        CODE,
        "现场态 public_url 与站点域名一致",
        format!("{}（变量来源 {}）", spec.declaration, vars.source),
    )]
}

pub fn expected_public_url(
    vars: &Vars,
    profile: &Profile,
    spec: &crate::profile::PublicUrlSpec,
) -> Option<String> {
    let host_spec = profile
        .domains
        .iter()
        .find(|domain| domain.var == spec.host_var)?;
    let host = domain_value(vars, host_spec)?;
    if host.trim().is_empty() {
        return None;
    }
    let port = vars.get(spec.port_var).unwrap_or(spec.default_port);
    if port == "443" {
        Some(format!("https://{host}"))
    } else {
        Some(format!("https://{host}:{port}"))
    }
}

pub fn normalize_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

// ── 9. 数据库初始化脚本 ───────────────────────────────────────────────────

/// `db-schema` 每次 start 都会读 `01_schema.sql`；文件丢了不是「以后再说」，是本次启动就坏。
pub fn database_initdb(stack: &Stack, profile: &Profile) -> Vec<Finding> {
    const CODE: &str = "database.initdb";
    let Some(schema_file) = profile.schema_file else {
        return Vec::new();
    };
    let path = stack.at(schema_file);
    match fs::metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() > 0 => vec![Finding::ok(
            CODE,
            "建表脚本就位",
            schema_file.to_string(),
        )],
        Ok(_) => vec![
            Finding::fail(CODE, "建表脚本为空", schema_file.to_string())
                .with_hint(format!("从版本包恢复 {schema_file}")),
        ],
        Err(err) => vec![
            Finding::fail(
                CODE,
                "缺少建表脚本",
                format!("{}：{err}", path.display()),
            )
            .with_hint(format!("每次 start 都会读它；从版本包恢复 {schema_file}")),
        ],
    }
}

// ── 10. 宿主端口 ──────────────────────────────────────────────────────────

/// 一个发布到宿主的端口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedPort {
    pub host_port: u16,
    pub protocol: String,
    pub bind_ip: Option<String>,
}

/// 取 compose 里**所有**同名块键（`ports:`）下缩进更深的行。
pub fn compose_blocks<'a>(compose: &'a str, key: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut block_indent: Option<usize> = None;
    let header = format!("{key}:");
    for line in compose.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();
        let is_header = trimmed == header || trimmed.starts_with(&header);
        match block_indent {
            Some(base) if indent > base => {
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    out.push(line);
                }
            }
            Some(_) | None => {
                block_indent = is_header.then_some(indent);
            }
        }
    }
    out
}

pub fn compose_ports(compose: &str, vars: &BTreeMap<String, String>) -> Vec<PublishedPort> {
    let mut out = Vec::new();

    // 行内写法：`ports: ["127.0.0.1:3100:3100"]`。块写法不会命中这里（冒号后为空）。
    for line in compose.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("ports:") else {
            continue;
        };
        let rest = rest.trim();
        if rest.is_empty() || rest.starts_with('{') {
            continue;
        }
        let list = rest.trim_start_matches('[').trim_end_matches(']');
        for item in list.split(',') {
            if let Some(port) = parse_short_port(item) {
                out.push(port);
            }
        }
    }

    let lines = compose_blocks(compose, "ports");
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index].trim();
        index += 1;
        let Some(item) = line.strip_prefix("- ") else {
            continue; // 长语法的后续键，已在上一个条目里处理
        };
        // 长语法：`- target: 3000` 加后续 `published: 443` / `host_ip: 127.0.0.1` / `protocol: udp`
        if item.contains("target:") || item.contains("published:") {
            let mut published = None;
            let mut protocol = "tcp".to_string();
            let mut bind_ip = None;
            let mut scan = |text: &str| {
                if let Some((_, value)) = text.split_once("published:")
                    && let Ok(port) = value.trim().trim_matches('"').trim_end_matches(',').parse()
                {
                    published = Some(port);
                }
                if let Some((_, value)) = text.split_once("protocol:") {
                    protocol = value
                        .trim()
                        .trim_matches('"')
                        .trim_end_matches(',')
                        .to_string();
                }
                if let Some((_, value)) = text.split_once("host_ip:") {
                    let value = value.trim().trim_matches('"').trim_end_matches(',');
                    if !value.is_empty() {
                        bind_ip = Some(value.to_string());
                    }
                }
            };
            scan(item);
            while index < lines.len() && !lines[index].trim().starts_with("- ") {
                scan(lines[index]);
                index += 1;
            }
            if let Some(host_port) = published {
                out.push(PublishedPort {
                    host_port,
                    protocol,
                    bind_ip,
                });
            }
            continue;
        }
        // 短语法：`"[ip:]host:container[/proto]"`
        let expanded = expand(item.trim().trim_matches('"'), vars);
        if let Some(port) = parse_short_port(&expanded) {
            out.push(port);
        }
    }
    out
}

/// 短语法 `"[ip:]host:container[/proto]"`。
///
/// IPv6 的宿主地址必须带方括号（`[::1]:8080:80`）：直接按 `:` 切会把它
/// 错当成「段数不对」而**静默跳过**，于是端口冲突永远查不出来。
pub fn parse_short_port(raw: &str) -> Option<PublishedPort> {
    let owned = raw.trim().trim_matches('"').trim_matches('\'');
    let (spec, protocol) = match owned.split_once('/') {
        Some((spec, protocol)) => (spec, protocol.to_string()),
        None => (owned, "tcp".to_string()),
    };

    let (bracket_ip, rest) = match spec.strip_prefix('[') {
        Some(after) => {
            let (ip, tail) = after.split_once(']')?;
            (Some(ip.to_string()), tail.strip_prefix(':')?)
        }
        None => (None, spec),
    };

    let parts: Vec<&str> = rest.split(':').collect();
    // 2 段时**第一段**才是宿主端口 —— 取错就会拿容器端口去查宿主，永远查不出冲突。
    let (host, bind_ip) = match (parts.as_slice(), bracket_ip) {
        ([host, _container], None) => (*host, None),
        ([ip, host, _container], None) => (*host, Some((*ip).to_string())),
        ([host, _container], Some(ip)) => (*host, Some(ip)),
        _ => return None, // 只写了容器端口 / 形状不认识 = 不发布到宿主
    };
    let host_port = host.trim().parse::<u16>().ok()?;
    Some(PublishedPort {
        host_port,
        protocol,
        bind_ip,
    })
}

pub fn compose_images(compose: &str, vars: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for line in compose.lines() {
        let trimmed = line.trim();
        let Some(value) = trimmed.strip_prefix("image:") else {
            continue;
        };
        let image = expand(value.trim().trim_matches('"').trim_matches('\''), vars);
        // 同一镜像常被多个一次性任务复用（db-init / db-schema 都用 postgres）：
        // 去重后再并行探，避免同一次体检对一个 registry 发多份请求。
        if !image.is_empty() && !out.contains(&image) {
            out.push(image);
        }
    }
    out
}

/// 解析 `docker ps --format '{{.Ports}}'` 的发布端口（`0.0.0.0:443->3000/tcp, …`）。
///
/// **必须同时按逗号和换行切**：`--format` 每个容器一行，行之间没有逗号；
/// 只按逗号切会把「上一行最后一个映射 + 下一行第一个映射」并成一块，
/// 而 `split_once("->")` 只取第一个 —— 下一行第一个发布的端口会被静默漏掉。
pub fn published_ports_from_ps(output: &str) -> std::collections::HashSet<u16> {
    let mut ours = std::collections::HashSet::new();
    for chunk in output.split([',', '\n']) {
        let Some((host_side, _)) = chunk.split_once("->") else {
            continue;
        };
        let Some((_, port)) = host_side.trim().rsplit_once(':') else {
            continue;
        };
        if let Ok(port) = port.parse::<u16>() {
            ours.insert(port);
        }
    }
    ours
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortState {
    Free,
    InUse,
    Unknown,
}

pub fn port_state(bind_ip: Option<&str>, port: u16) -> PortState {
    let try_bind = |addr: &str| -> PortState {
        match std::net::TcpListener::bind((addr, port)) {
            Ok(_) => PortState::Free,
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => PortState::InUse,
            Err(_) => PortState::Unknown,
        }
    };

    // 先按 compose 里写的 bind IP 试：中心多数端口只绑 127.0.0.1。
    // 绑不上（IP 不在本机 / 权限）时回落到通配，别把「地址不可用」误报成「端口被占」。
    if let Some(ip) = bind_ip.filter(|ip| !ip.is_empty() && *ip != "0.0.0.0") {
        match try_bind(ip) {
            PortState::Free => return PortState::Free,
            PortState::InUse => return PortState::InUse,
            PortState::Unknown => {}
        }
    }
    try_bind("0.0.0.0")
}

/// compose 的**项目名**（容器标签 `com.docker.compose.project` 的值）。
///
/// 不用 `project.working_dir`：栈目录搬过位置后，仍在跑的容器带的是**旧路径**标签，
/// 按路径过滤会漏掉自己。项目名跨路径稳定。
pub fn compose_project_name(stack: &Stack, compose: &str, profile: &Profile) -> String {
    if let Ok(value) = std::env::var("COMPOSE_PROJECT_NAME")
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    for line in compose.lines() {
        // 只认**顶格**的 `name:`（项目名）。缩进的是 volumes/networks 里的名字。
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some(value) = line.strip_prefix("name:") else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !value.is_empty() {
            return value.to_string();
        }
        break;
    }
    stack
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(profile.project_fallback)
        .to_string()
}

pub fn our_published_ports(
    stack: &Stack,
    compose: &str,
    profile: &Profile,
) -> Option<std::collections::HashSet<u16>> {
    let filter = format!(
        "label=com.docker.compose.project={}",
        compose_project_name(stack, compose, profile)
    );
    let (ok, stdout, _) = run_with_timeout(
        "docker",
        &["ps", "--filter", &filter, "--format", "{{.Ports}}"],
        10,
    )?;
    ok.then(|| published_ports_from_ps(&stdout))
}

/// 发布到宿主的端口是否真的空着。
///
/// 必须排除**本栈自己**占着的端口：升级的 `localize` 阶段容器正在跑，
/// 不排除就会每次误报。
pub fn ports_available(stack: &Stack, vars: &Vars, profile: &Profile) -> Vec<Finding> {
    const CODE: &str = "port.available";
    if !vars.ok() {
        return Vec::new();
    }
    let Some(compose_path) = compose_file(stack) else {
        return Vec::new(); // 缺 compose 已由 layout.mounts 报
    };
    let Ok(compose) = fs::read_to_string(&compose_path) else {
        return vec![Finding::warn(CODE, "读不到 compose，跳过端口检查", "")];
    };
    let ours = our_published_ports(stack, &compose, profile);
    ports_available_with(stack, vars, profile, &compose, ours.as_ref())
}

/// 可注入「本栈已发布的端口」的版本：测试里不需要真的起 docker。
pub fn ports_available_with(
    stack: &Stack,
    vars: &Vars,
    profile: &Profile,
    compose: &str,
    ours: Option<&std::collections::HashSet<u16>>,
) -> Vec<Finding> {
    const CODE: &str = "port.available";
    let _ = (stack, profile);
    if !vars.ok() {
        return Vec::new();
    }
    let ports = compose_ports(compose, &vars.map);
    if ports.is_empty() {
        return vec![Finding::ok(CODE, "没有发布到宿主的端口", "")];
    }
    let Some(ours) = ours else {
        return vec![
            Finding::warn(
                CODE,
                "无法确认端口归属，跳过占用检查",
                "docker 不可用（没装或守护进程没起）",
            )
            .with_hint("栈跑着时端口本就被自己占着；手工确认用 `docker ps` / `lsof -i`"),
        ];
    };

    let mut conflicts = Vec::new();
    let mut unknown = Vec::new();
    let mut skipped = 0usize;
    for port in &ports {
        if port.protocol != "tcp" || ours.contains(&port.host_port) {
            skipped += 1;
            continue;
        }
        match port_state(port.bind_ip.as_deref(), port.host_port) {
            PortState::InUse => conflicts.push(port.host_port),
            PortState::Unknown => unknown.push(port.host_port),
            PortState::Free => {}
        }
    }
    conflicts.sort_unstable();
    conflicts.dedup();

    if !conflicts.is_empty() {
        return vec![
            Finding::fail(
                CODE,
                "宿主端口已被别的进程占用",
                format!(
                    "占用中：{}",
                    conflicts
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            )
            .with_hint("`compose up` 会失败（port is already allocated）；腾端口或改 *_PORT"),
        ];
    }
    if !unknown.is_empty() {
        unknown.sort_unstable();
        unknown.dedup();
        return vec![
            Finding::warn(
                CODE,
                "部分端口无法判定（bind IP 不在本机或无权限）",
                format!(
                    "无法判定：{}",
                    unknown
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            )
            .with_hint("确认 *_BIND_IP 是本机网卡地址；`compose up` 前用 `lsof -i` 核对"),
        ];
    }
    let checked = ports.len() - skipped;
    if checked == 0 {
        return vec![Finding::ok(
            CODE,
            "端口都由本栈自己占着，跳过占用检查",
            format!("{} 个发布端口（栈在运行）", ports.len()),
        )];
    }
    vec![Finding::ok(
        CODE,
        "宿主端口都空着",
        format!(
            "已检查 {checked}/{}（{} 个由本栈占着 / udp）",
            ports.len(),
            skipped
        ),
    )]
}

pub fn run_with_timeout(program: &str, args: &[&str], secs: u64) -> Option<(bool, String, String)> {
    use std::process::{Command, Stdio};

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                return Some((
                    status.success(),
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Some((false, String::new(), format!("超时（{secs}s）")));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(err) => return Some((false, String::new(), err.to_string())),
        }
    }
}

// ── 11. 镜像可达 ──────────────────────────────────────────────────────────

/// 网络类错误（整个站点离线时会全部命中）—— 收敛成一条提示，不按镜像刷屏。
pub fn looks_like_network_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    [
        "no such host",
        "timeout",
        "超时",
        "connection refused",
        "temporary failure in name resolution",
        "network is unreachable",
        "i/o timeout",
        "tls handshake timeout",
        "context deadline exceeded",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

pub fn image_reachable(
    stack: &Stack,
    vars: &Vars,
    profile: &Profile,
    offline: bool,
) -> Vec<Finding> {
    const CODE: &str = "image.reachable";
    let _ = profile;
    if offline {
        return vec![Finding::ok(CODE, "跳过镜像可达检查（--offline）", "")];
    }
    if !vars.ok() {
        return Vec::new();
    }
    let Some(compose_path) = compose_file(stack) else {
        return Vec::new();
    };
    let Ok(compose) = fs::read_to_string(&compose_path) else {
        return Vec::new();
    };
    let images = compose_images(&compose, &vars.map);
    if images.is_empty() {
        return vec![Finding::ok(CODE, "compose 里没有镜像", "")];
    }

    image_reachable_with(&images, &|image| {
        // 本地已有 → 不需要网络（`pull` 也会直接跳过）；这一步 55ms，远端 manifest 可能 34s。
        if let Some((true, _, _)) = run_with_timeout("docker", &["image", "inspect", image], 5) {
            return Ok(());
        }
        let Some((ok, _, stderr)) = run_with_timeout("docker", &["manifest", "inspect", image], 6)
        else {
            return Err("docker 不可用".to_string());
        };
        if ok {
            Ok(())
        } else {
            Err(stderr
                .trim()
                .lines()
                .next()
                .unwrap_or("未知错误")
                .to_string())
        }
    })
}

pub fn image_reachable_with(
    images: &[String],
    probe: &(dyn Fn(&str) -> Result<(), String> + Sync),
) -> Vec<Finding> {
    const CODE: &str = "image.reachable";
    let mut broken = Vec::new();
    let mut offline = 0usize;
    // 并行探测：串行时一个慢 registry 会把体检拖成几十秒（实测单个 34s）。
    let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = images
            .iter()
            .map(|image| scope.spawn(move || probe(image)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("探测线程崩溃".to_string()))
            })
            .collect()
    });
    for (image, result) in images.iter().zip(results) {
        match result {
            Ok(()) => {}
            Err(err) if looks_like_network_error(&err) => offline += 1,
            Err(err) => broken.push(format!("{image}（{err}）")),
        }
    }

    if !broken.is_empty() {
        let shown = broken
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("；");
        return vec![
            Finding::warn(
                CODE,
                "镜像在源上取不到",
                format!("{} 个：{shown}", broken.len()),
            )
            .non_blocking()
            .with_hint(
                "确认 tag 与镜像源前缀（*_REGISTRY / *_IMAGE_TAG）；`compose pull` 会失败在这里",
            ),
        ];
    }
    if offline == images.len() {
        return vec![
            Finding::warn(
                CODE,
                "镜像源不可达（离线？）",
                format!("{offline} 个镜像全部拿不到 manifest"),
            )
            .with_hint("离线站点用 `--offline` 跳过本项，免得每次体检都报"),
        ];
    }
    vec![Finding::ok(
        CODE,
        "镜像在源上都能取到",
        format!("{} 个", images.len()),
    )]
}

// ── 测试 ────────────────────────────────────────────────────────────────────
