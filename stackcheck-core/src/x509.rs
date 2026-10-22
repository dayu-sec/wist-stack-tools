//! 证书 SAN / 有效期解析（只读 PEM，不验签）。
//!
//! 为什么自己解析：原来脚本里的 `openssl x509 -ext | grep -q "DNS:${domain}"` 是**子串**匹配，
//! 正则里的 `.` 还是通配符 —— SAN 为 `gw.example.com.evil.net` 也会判过。
//! `x509-parser` 不依赖外部命令，也不受 LibreSSL / OpenSSL 差异影响。

use std::net::IpAddr;

/// 叶证书里与判据相关的字段。
#[derive(Debug, Clone)]
pub struct LeafCert {
    pub dns: Vec<String>,
    pub ips: Vec<IpAddr>,
    pub not_before: String,
    pub not_after: String,
    pub valid: bool,
}

/// 解析 PEM 叶证书。
pub fn parse_pem(bytes: &[u8]) -> Result<LeafCert, String> {
    let (_, pem) =
        x509_parser::pem::parse_x509_pem(bytes).map_err(|_| "不是合法的 PEM".to_string())?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)
        .map_err(|_| "证书无法解析".to_string())?;
    let validity = cert.validity();
    Ok(LeafCert {
        dns: certificate_dns_names(&cert),
        ips: certificate_ip_names(&cert),
        not_before: validity.not_before.to_string(),
        not_after: validity.not_after.to_string(),
        valid: validity.is_valid(),
    })
}

/// 证书是否覆盖某个域名：
/// - 精确相等；
/// - 通配符只覆盖**一层**（`*.a.b` 覆盖 `x.a.b`，不覆盖 `a.b` / `x.y.a.b`）；
/// - IPv4 / IPv6（允许 `[::1]` 方括号写法）按 IP SAN 比较。
pub fn covers(leaf: &LeafCert, domain: &str) -> bool {
    let domain = domain.trim();
    let ip_candidate = domain
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(domain);
    match ip_candidate.parse::<IpAddr>() {
        Ok(ip) => leaf.ips.contains(&ip),
        Err(_) => leaf.dns.iter().any(|san| san_covers(san, domain)),
    }
}

/// SAN 列表的展示串。
pub fn san_summary(leaf: &LeafCert) -> String {
    let mut out = leaf.dns.clone();
    out.extend(leaf.ips.iter().map(ToString::to_string));
    out.join(", ")
}

pub fn certificate_dns_names(cert: &x509_parser::prelude::X509Certificate<'_>) -> Vec<String> {
    cert.subject_alternative_name()
        .ok()
        .flatten()
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    x509_parser::extensions::GeneralName::DNSName(dns) => Some((*dns).to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn certificate_ip_names(
    cert: &x509_parser::prelude::X509Certificate<'_>,
) -> Vec<std::net::IpAddr> {
    cert.subject_alternative_name()
        .ok()
        .flatten()
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    x509_parser::extensions::GeneralName::IPAddress(bytes) => ip_from_bytes(bytes),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn ip_from_bytes(bytes: &[u8]) -> Option<std::net::IpAddr> {
    match bytes.len() {
        4 => Some(std::net::IpAddr::from([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])),
        16 => {
            let mut raw = [0u8; 16];
            raw.copy_from_slice(bytes);
            Some(std::net::IpAddr::from(raw))
        }
        _ => None,
    }
}

/// SAN 是否覆盖某个域名：**精确相等**，或通配符只覆盖**一层**。
pub fn san_covers(san: &str, domain: &str) -> bool {
    let san = san.trim().trim_end_matches('.').to_ascii_lowercase();
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if san == domain {
        return true;
    }
    let Some(suffix) = san.strip_prefix("*.") else {
        return false;
    };
    domain
        .strip_suffix(suffix)
        .and_then(|head| head.strip_suffix('.'))
        .is_some_and(|label| !label.is_empty() && !label.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_only_covers_one_label() {
        assert!(san_covers("*.a.b", "x.a.b"));
        assert!(!san_covers("*.a.b", "a.b"));
        assert!(!san_covers("*.a.b", "x.y.a.b"));
        assert!(!san_covers("gw.a.b", "gw.a.b.evil.net"));
        assert!(san_covers("GW.A.B.", "gw.a.b"));
    }
}
