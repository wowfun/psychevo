#[allow(unused_imports)]
pub(crate) use super::*;

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use futures::{Stream, StreamExt};

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
        require_public_addresses(&addresses)?;
        Ok(ValidatedWebUrl {
            url,
            host,
            addresses,
        })
    }
}

fn require_public_addresses(addresses: &[SocketAddr]) -> Result<()> {
    if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
        return Err(Error::Message("web URL target is not public".into()));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct PublicHttpGetOptions {
    pub(crate) timeout: Duration,
    pub(crate) redirect_limit: usize,
    pub(crate) user_agent: &'static str,
    pub(crate) accept: &'static str,
    pub(crate) accept_language: Option<&'static str>,
    pub(crate) operation: &'static str,
}

pub(crate) async fn public_http_get(
    raw: &str,
    options: &PublicHttpGetOptions,
    mut abort: Option<AbortSignal>,
) -> Result<reqwest::Response> {
    let policy = WebUrlPolicy;
    let mut validated = policy.validate(raw).await?;
    for redirect_count in 0..=options.redirect_limit {
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(options.timeout);
        let client = pin_validated_addresses(builder, &validated).build()?;
        let request = build_public_http_request(&client, &validated, options);
        let response = match abort.as_mut() {
            Some(abort) => {
                tokio::select! {
                    _ = abort.wait_for_abort() => {
                        return Err(Error::Message(format!("{} aborted", options.operation)));
                    }
                    result = request.send() => result?,
                }
            }
            None => request.send().await?,
        };
        if !response.status().is_redirection() {
            return Ok(response);
        }
        if redirect_count == options.redirect_limit {
            return Err(Error::Message(format!(
                "{} redirect limit exceeded",
                options.operation
            )));
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                Error::Message(format!(
                    "{} redirect did not contain a valid Location",
                    options.operation
                ))
            })?;
        validated = validate_redirect_target(&policy, &validated.url, location).await?;
    }
    unreachable!("redirect loop either returns a response or an error")
}

fn build_public_http_request(
    client: &reqwest::Client,
    validated: &ValidatedWebUrl,
    options: &PublicHttpGetOptions,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(validated.url.clone())
        .header(reqwest::header::USER_AGENT, options.user_agent)
        .header(reqwest::header::ACCEPT, options.accept);
    if let Some(accept_language) = options.accept_language {
        request = request.header(reqwest::header::ACCEPT_LANGUAGE, accept_language);
    }
    request
}

fn pin_validated_addresses(
    builder: reqwest::ClientBuilder,
    validated: &ValidatedWebUrl,
) -> reqwest::ClientBuilder {
    // Pin the connection to every address which passed the public-IP check. A
    // subsequent DNS answer cannot redirect a direct request, while normal
    // multi-address fallback remains available.
    builder.resolve_to_addrs(&validated.host, &validated.addresses)
}

async fn validate_redirect_target(
    policy: &WebUrlPolicy,
    base: &reqwest::Url,
    location: &str,
) -> Result<ValidatedWebUrl> {
    let next = base
        .join(location)
        .map_err(|_| Error::Message("web redirect URL is invalid".into()))?;
    policy.validate(next.as_str()).await
}

pub(crate) async fn read_bounded_http_response(
    response: reqwest::Response,
    max_bytes: usize,
    abort: Option<AbortSignal>,
    operation: &'static str,
) -> Result<Vec<u8>> {
    if let Some(length) = response.content_length()
        && length > max_bytes as u64
    {
        return Err(Error::Message(format!(
            "{operation} response too large: content-length {length} exceeds {max_bytes} bytes"
        )));
    }
    collect_bounded_stream(response.bytes_stream(), max_bytes, abort, operation).await
}

async fn collect_bounded_stream<S, B, E>(
    mut stream: S,
    max_bytes: usize,
    mut abort: Option<AbortSignal>,
    operation: &'static str,
) -> Result<Vec<u8>>
where
    S: Stream<Item = std::result::Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let mut output = Vec::new();
    loop {
        let item = match abort.as_mut() {
            Some(abort) => {
                tokio::select! {
                    _ = abort.wait_for_abort() => {
                        return Err(Error::Message(format!("{operation} aborted")));
                    }
                    item = stream.next() => item,
                }
            }
            None => stream.next().await,
        };
        let Some(chunk) = item else {
            return Ok(output);
        };
        let chunk = chunk
            .map_err(|error| Error::Message(format!("{operation} response failed: {error}")))?;
        let chunk = chunk.as_ref();
        if output.len().saturating_add(chunk.len()) > max_bytes {
            return Err(Error::Message(format!(
                "{operation} response too large: exceeds {max_bytes} bytes"
            )));
        }
        output.extend_from_slice(chunk);
    }
}

const MAX_NESTED_URL_DEPTH: usize = 4;
const MAX_INSPECTED_URLS: usize = 16;

pub(crate) fn validate_url_text(raw: &str) -> Result<()> {
    let url = reqwest::Url::parse(raw).map_err(|_| Error::Message("web URL is invalid".into()))?;
    let mut pending = VecDeque::from([(url, 0_usize)]);
    let mut inspected_urls = 1_usize;

    while let Some((url, depth)) = pending.pop_front() {
        for (_, value) in url.query_pairs() {
            let value = value.trim();
            if high_confidence_credential_value(value) {
                return Err(Error::Message(
                    "web URL query appears to contain a credential".into(),
                ));
            }

            let Ok(nested) = reqwest::Url::parse(value) else {
                continue;
            };
            if !nested.username().is_empty() || nested.password().is_some() {
                return Err(Error::Message(
                    "web URL query appears to contain a credential".into(),
                ));
            }
            if depth >= MAX_NESTED_URL_DEPTH || inspected_urls >= MAX_INSPECTED_URLS {
                return Err(Error::Message(
                    "web URL nested query inspection limit exceeded".into(),
                ));
            }
            inspected_urls += 1;
            pending.push_back((nested, depth + 1));
        }
    }
    Ok(())
}

fn high_confidence_credential_value(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    if lower.contains("-----begin private key-----") {
        return true;
    }
    if lower.starts_with("bearer ") {
        return bearer_token(value["bearer ".len()..].trim());
    }
    ["sk-", "ghp_", "xoxb-", "github_pat_"]
        .into_iter()
        .any(|prefix| {
            lower
                .strip_prefix(prefix)
                .is_some_and(|suffix| credential_token_suffix(suffix, 20))
        })
}

fn bearer_token(value: &str) -> bool {
    let non_padding_len = value.bytes().take_while(|byte| *byte != b'=').count();
    non_padding_len >= 16
        && value[..non_padding_len].bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
        })
        && value[non_padding_len..].bytes().all(|byte| byte == b'=')
}

fn credential_token_suffix(value: &str, minimum_len: usize) -> bool {
    value.len() >= minimum_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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
    fn credential_policy_uses_high_confidence_query_values() {
        let slack_credential_url = format!(
            "https://example.com/?value={}{}",
            "xoxb-", "1234567890-abcdefghijklmnopqrstuvwxyz"
        );
        for url in [
            "https://example.com/task-list",
            "https://example.com/sk-this-is-an-ordinary-path-segment",
            "https://example.com/share?token=public-share&sig=link-signature",
            "https://example.com/docs?session=morning&key=reference",
        ] {
            assert!(validate_url_text(url).is_ok(), "{url}");
        }

        for url in [
            "https://example.com/?value=sk-abcdefghijklmnopqrstuvwxyz012345",
            "https://example.com/?value=SK-ABCDEFGHIJKLMNOPQRSTUVWXYZ012345",
            "https://example.com/?value=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            slack_credential_url.as_str(),
            "https://example.com/?value=github_pat_abcdefghijklmnopqrstuvwxyz0123456789",
            "https://example.com/?auth=Bearer%20high-confidence-credential",
            "https://example.com/?auth=Bearer%20abcdefghijklmnop%2B%2F~%3D%3D",
            "https://example.com/?pem=-----BEGIN%20PRIVATE%20KEY-----",
        ] {
            assert!(validate_url_text(url).is_err(), "{url}");
        }
    }

    #[test]
    fn credential_policy_recursively_inspects_absolute_url_query_values() {
        let nested_once = wrap_nested_url(
            "https://nested.example/callback?auth=sk-abcdefghijklmnopqrstuvwxyz012345",
        );
        assert!(
            validate_url_text(&nested_once).is_err(),
            "one nested URL"
        );

        let nested_twice = wrap_nested_url(&wrap_nested_url(
            "https://nested.example/callback?auth=Bearer%20abcdefghijklmnop%2B%2F%3D",
        ));
        assert!(
            validate_url_text(&nested_twice).is_err(),
            "two nested URLs"
        );

        let nested_userinfo = wrap_nested_url("https://user:password@nested.example/callback");
        assert!(
            validate_url_text(&nested_userinfo).is_err(),
            "nested userinfo"
        );

        let ordinary_share = wrap_nested_url(&wrap_nested_url(
            "https://nested.example/share?token=public-share&sig=link-signature",
        ));
        assert!(
            validate_url_text(&ordinary_share).is_ok(),
            "ordinary nested share URL"
        );
    }

    #[test]
    fn credential_policy_fails_closed_at_nested_url_budgets() {
        let mut too_deep = "https://nested.example/share".to_string();
        for _ in 0..=MAX_NESTED_URL_DEPTH {
            too_deep = wrap_nested_url(&too_deep);
        }
        let error = validate_url_text(&too_deep).expect_err("depth budget");
        assert!(error.to_string().contains("inspection limit"), "{error}");

        let mut fanout = reqwest::Url::parse("https://example.com/share").expect("outer URL");
        {
            let mut query = fanout.query_pairs_mut();
            for index in 0..MAX_INSPECTED_URLS {
                query.append_pair(
                    &format!("next{index}"),
                    &format!("https://nested{index}.example/share"),
                );
            }
        }
        let error = validate_url_text(fanout.as_str()).expect_err("total URL budget");
        assert!(error.to_string().contains("inspection limit"), "{error}");
    }

    fn wrap_nested_url(nested: &str) -> String {
        let mut outer = reqwest::Url::parse("https://example.com/share").expect("outer URL");
        outer.query_pairs_mut().append_pair("next", nested);
        outer.into()
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

    #[test]
    fn rejects_mixed_public_private_dns_answers_before_connect() {
        let addresses = [
            "1.1.1.1:443".parse().expect("public"),
            "127.0.0.1:443".parse().expect("private"),
        ];
        let error = require_public_addresses(&addresses).expect_err("mixed answer");
        assert!(error.to_string().contains("not public"), "{error}");
    }

    #[test]
    fn public_http_request_applies_the_selected_navigation_headers() {
        let validated = ValidatedWebUrl {
            url: reqwest::Url::parse("https://example.com/article").expect("url"),
            host: "example.com".to_string(),
            addresses: vec!["1.1.1.1:443".parse().expect("address")],
        };
        let options = PublicHttpGetOptions {
            timeout: Duration::from_secs(30),
            redirect_limit: 10,
            user_agent: "browser-compatible-agent",
            accept: "text/plain;q=1.0, text/html;q=0.8",
            accept_language: Some("en-US,en;q=0.9"),
            operation: "test",
        };
        let client = pin_validated_addresses(reqwest::Client::builder().no_proxy(), &validated)
            .build()
            .expect("client");

        let request = build_public_http_request(&client, &validated, &options)
            .build()
            .expect("request");
        assert_eq!(
            request.headers().get(reqwest::header::USER_AGENT),
            Some(&reqwest::header::HeaderValue::from_static(
                "browser-compatible-agent"
            ))
        );
        assert_eq!(
            request.headers().get(reqwest::header::ACCEPT),
            Some(&reqwest::header::HeaderValue::from_static(
                "text/plain;q=1.0, text/html;q=0.8"
            ))
        );
        assert_eq!(
            request.headers().get(reqwest::header::ACCEPT_LANGUAGE),
            Some(&reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"))
        );
    }

    #[tokio::test]
    async fn redirect_target_is_revalidated_before_following() {
        let policy = WebUrlPolicy;
        let base = reqwest::Url::parse("https://example.com/image.png").expect("base");
        let error = validate_redirect_target(&policy, &base, "http://127.0.0.1/private")
            .await
            .expect_err("private redirect");
        assert!(error.to_string().contains("not public"), "{error}");
    }

    #[tokio::test]
    async fn direct_transport_retains_every_validated_address() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let reachable = listener.local_addr().expect("reachable address");
        let unreachable_before =
            SocketAddr::new("127.0.0.2".parse().expect("loopback"), reachable.port());
        let unreachable_after =
            SocketAddr::new("127.0.0.3".parse().expect("loopback"), reachable.port());
        let validated = ValidatedWebUrl {
            url: reqwest::Url::parse("http://multi-address.test/").expect("url"),
            host: "multi-address.test".to_string(),
            addresses: vec![unreachable_before, reachable, unreachable_after],
        };
        let client = pin_validated_addresses(reqwest::Client::builder().no_proxy(), &validated)
            .build()
            .expect("client");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.expect("read request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                )
                .await
                .expect("write response");
        });

        let response = client
            .get(validated.url)
            .send()
            .await
            .expect("reachable validated address");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.expect("body"), "ok");
        server.await.expect("server");
    }

    #[tokio::test]
    async fn bounded_stream_stops_before_exceeding_the_limit() {
        let stream = futures::stream::iter([
            Ok::<_, std::convert::Infallible>(b"1234".as_slice()),
            Ok(b"5678".as_slice()),
            Ok(b"9".as_slice()),
        ]);
        let error = collect_bounded_stream(stream, 8, None, "test")
            .await
            .expect_err("bounded");
        assert!(error.to_string().contains("exceeds 8 bytes"), "{error}");
    }
}
