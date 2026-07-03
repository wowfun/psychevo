#[allow(unused_imports)]
pub(crate) use super::*;

#[path = "config/providers.rs"]
mod providers;
#[allow(unused_imports)]
pub use providers::*;
#[path = "config/resolution.rs"]
mod resolution;
#[allow(unused_imports)]
pub use resolution::*;
#[path = "config/channels.rs"]
mod channels;
#[allow(unused_imports)]
pub use channels::*;

#[test]
fn profile_mcp_servers_parse_stdio_and_http_descriptors() {
    let config = crate::config::config_parse::parse_run_config(json!({
        "mcp_servers": {
            "repo tools": {
                "command": "./mcp-server",
                "args": ["--stdio"],
                "env": { "MCP_MODE": "test" },
                "cwd": "./tools"
            },
            "docs": {
                "transport": "streamable_http",
                "url": "https://example.test/mcp",
                "headers": { "x-test": "yes" }
            }
        }
    }))
    .expect("config");

    assert_eq!(config.mcp_servers.len(), 2);
    let repo = config
        .mcp_servers
        .iter()
        .find(|server| server.name == "repo tools")
        .expect("repo tools");
    let docs = config
        .mcp_servers
        .iter()
        .find(|server| server.name == "docs")
        .expect("docs");
    assert_eq!(repo.source_kind.as_deref(), Some("profile"));
    assert!(matches!(
        &repo.transport,
        crate::types::McpTransportInput::Stdio { args, env, cwd, .. }
            if args == &vec!["--stdio".to_string()]
                && env.get("MCP_MODE").map(String::as_str) == Some("test")
                && cwd.as_deref() == Some(std::path::Path::new("./tools"))
    ));
    assert!(matches!(
        &docs.transport,
        crate::types::McpTransportInput::StreamableHttp { url, headers }
            if url == "https://example.test/mcp"
                && headers.get("x-test").map(String::as_str) == Some("yes")
    ));
}
