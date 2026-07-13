#[allow(unused_imports)]
pub(crate) use super::*;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

#[derive(Debug, Clone)]
pub(crate) struct ValidatedWebUrl {
    pub(crate) url: reqwest::Url,
    pub(crate) host: String,
    pub(crate) addresses: Vec<SocketAddr>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WebUrlPolicy;

impl WebUrlPolicy {
    pub(crate) async fn validate(&self, raw: &str) -> Result<ValidatedWebUrl> {
        validate_url_text(raw)?;
        let url =
            reqwest::Url::parse(raw).map_err(|_| Error::Message("web URL is invalid".into()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::Message(
                "web URL scheme must be http or https".into(),
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Error::Message(
                "web URL must not contain credentials".into(),
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| Error::Message("web URL must contain a host".into()))?
            .to_string();
        if host.eq_ignore_ascii_case("localhost")
            || host.to_ascii_lowercase().ends_with(".localhost")
        {
            return Err(Error::Message("web URL target is not public".into()));
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| Error::Message("web URL port is invalid".into()))?;
        let addresses = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            tokio::net::lookup_host((host.as_str(), port))
                .await
                .map_err(|_| Error::Message("web URL DNS lookup failed".into()))?
                .collect::<Vec<_>>()
        };
        if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
            return Err(Error::Message("web URL target is not public".into()));
        }
        Ok(ValidatedWebUrl {
            url,
            host,
            addresses,
        })
    }
}

pub(crate) fn validate_url_text(raw: &str) -> Result<()> {
    let decoded = percent_decode(raw)?;
    let lower = decoded.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "sk-",
        "xoxb-",
        "ghp_",
        "github_pat_",
        "-----begin private key",
    ] {
        if lower.contains(marker) {
            return Err(Error::Message(
                "web URL appears to contain a credential".into(),
            ));
        }
    }
    let url = reqwest::Url::parse(raw).map_err(|_| Error::Message("web URL is invalid".into()))?;
    for (name, _) in url.query_pairs() {
        let normalized = name.to_ascii_lowercase().replace('-', "_");
        if [
            "token",
            "access_token",
            "refresh_token",
            "api_key",
            "apikey",
            "key",
            "secret",
            "client_secret",
            "password",
            "passwd",
            "authorization",
            "auth",
            "credential",
            "signature",
            "sig",
            "jwt",
            "session",
        ]
        .contains(&normalized.as_str())
        {
            return Err(Error::Message(format!(
                "web URL contains sensitive query parameter `{name}`"
            )));
        }
    }
    Ok(())
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).and_then(|byte| hex(*byte));
            let low = bytes.get(index + 2).and_then(|byte| hex(*byte));
            let (Some(high), Some(low)) = (high, low) else {
                return Err(Error::Message(
                    "web URL has invalid percent encoding".into(),
                ));
            };
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| Error::Message("web URL percent-decoded text is not UTF-8".into()))
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_ipv4(ip),
        IpAddr::V6(ip) => public_ipv6(ip),
    }
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

fn public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return public_ipv4(ipv4);
    }
    let segments = ip.segments();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        && (segments[0] & 0xe000) == 0x2000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_sensitive_and_encoded_sensitive_urls() {
        assert!(validate_url_text("https://example.com/?api_key=secret").is_err());
        assert!(validate_url_text("https://example.com/%73%6b%2dsecret").is_err());
        assert!(validate_url_text("https://example.com/docs").is_ok());
    }

    #[test]
    fn rejects_non_public_address_classes() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "224.0.0.1",
            "0.0.0.0",
            "192.0.2.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("1.1.1.1".parse().unwrap()));
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
    }
}
