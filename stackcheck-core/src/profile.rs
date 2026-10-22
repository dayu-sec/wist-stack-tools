//! 判据参数（`StackProfile`）。
//!
//! 检查逻辑只认这里的「声明」：路径、变量名、挂载对、必填身份文件。网关栈与中心栈
//! 各自提供一份 profile，检查器本身不出现任何栈专属路径。

/// 渲染产物 ↔ 值文件（或模板）。产物 mtime 早于来源 = 改了值没重渲。
pub struct RenderPair {
    pub code: &'static str,
    pub rendered: &'static str,
    pub source: &'static str,
}

/// 文件缺失时的处理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// 未生成（例如尚未 localize）：不报。
    Skip,
    /// 该有但没有：WARN。
    Warn,
    /// 缺了就会坏：FAIL。
    Fail,
}

/// 不该对其他用户开放的文件（含口令 / token）。
pub struct SecretFile {
    pub path: &'static str,
    pub label: &'static str,
    pub missing: Missing,
    /// 命中（缺文件 / world 位）时是否阻断部署。安全类问题通常阻断；配置文件仅提示。
    pub blocking: bool,
}

/// 站点身份材料。
pub struct IdentityFile {
    pub path: &'static str,
    pub label: &'static str,
    /// 缺失是否阻断部署：叶证书/私钥/信任根会直接坏；CA 私钥与 CA-G 只影响以后的操作。
    pub blocking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountKind {
    File,
    Dir,
}

/// compose bind 挂载 ↔ 变量 ↔ 实际落点。
pub struct MountSpec {
    /// compose 里**字面**出现的源（含 `${VAR}`），例如 `${CENTER_CONFIG_DIR}/wist-center.toml`。
    pub source_expr: &'static str,
    /// 容器内目标路径。
    pub target: &'static str,
    /// localize 实际写入 / 创建的宿主路径（相对栈根）。
    pub expected_host: &'static str,
    pub kind: MountKind,
}

/// 一个必须由现场给出（或可派生）的域名变量。
pub struct DomainSpec {
    pub var: &'static str,
    /// 为空时的派生规则，例如 `api.<WEB_DOMAIN>`。
    pub derive: Option<DeriveSpec>,
}

pub struct DeriveSpec {
    pub from: &'static str,
    /// `{}` 会替换成 `from` 的值。
    pub pattern: &'static str,
}

/// 现场态 `public_url`：声明（只写 `${VAR}`）、值（`.env`）、主配置（不得重复定义）。
pub struct PublicUrlSpec {
    pub main_config: &'static str,
    pub declaration: &'static str,
    pub env: &'static str,
    pub env_var: &'static str,
    pub port_var: &'static str,
    pub default_port: &'static str,
    /// 期望 URL 用的域名变量（通常是网关面域名）。
    pub host_var: &'static str,
}

pub struct Profile {
    /// 包名 / compose 项目名回落值。
    pub project_fallback: &'static str,
    pub render_pairs: &'static [RenderPair],
    pub placeholder_files: &'static [&'static str],
    pub placeholder_patterns: &'static [&'static str],
    pub domains: &'static [DomainSpec],
    pub secret_files: &'static [SecretFile],
    pub private_key_roots: &'static [&'static str],
    pub identity_files: &'static [IdentityFile],
    pub mounts: &'static [MountSpec],
    /// PG 建表脚本（没有数据库的栈设为 `None`）。
    pub schema_file: Option<&'static str>,
    pub public_url: Option<PublicUrlSpec>,
    /// 站点服务器叶证书（相对栈根）；没有 TLS 面的栈设为 `None`。
    pub server_cert: Option<&'static str>,
}

pub const PLACEHOLDER_PATTERNS: &[&str] = &[
    "example.com",
    "warpinsight.example",
    "CHANGEME",
    "changeme",
];
