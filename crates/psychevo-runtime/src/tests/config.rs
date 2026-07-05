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
        "tools": {
            "tool_search": {
                "enabled": true,
                "default_limit": 3,
                "max_limit": 7
            }
        },
        "mcp_servers": {
            "repo tools": {
                "command": "./mcp-server",
                "args": ["--stdio"],
                "env": { "MCP_MODE": "test" },
                "cwd": "./tools",
                "enabled_tools": ["search"],
                "disabled_tools": ["delete"],
                "supports_parallel_tool_calls": true,
                "startup_timeout_secs": 2,
                "tool_timeout_secs": 5
            },
            "docs": {
                "transport": "streamable_http",
                "url": "https://example.test/mcp",
                "headers": { "x-test": "yes" },
                "bearer_token_env_var": "DOCS_MCP_TOKEN",
                "scopes": ["docs.read"],
                "oauth_resource": "https://auth.example.test",
                "oauth": { "client_id": "psychevo" }
            }
        }
    }))
    .expect("config");

    assert_eq!(config.tools.tool_search.default_limit, 3);
    assert_eq!(config.tools.tool_search.max_limit, 7);
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
    assert_eq!(
        repo.policy.enabled_tools.as_deref(),
        Some(&["search".to_string()][..])
    );
    assert_eq!(repo.policy.disabled_tools, vec!["delete".to_string()]);
    assert!(repo.policy.supports_parallel_tool_calls);
    assert_eq!(repo.policy.startup_timeout_secs, Some(2));
    assert_eq!(repo.policy.tool_timeout_secs, Some(5));
    assert!(matches!(
        &repo.transport,
        crate::types::McpTransportInput::Stdio { args, env, cwd, .. }
            if args == &vec!["--stdio".to_string()]
                && env.get("MCP_MODE").map(String::as_str) == Some("test")
                && cwd.as_deref() == Some(std::path::Path::new("./tools"))
    ));
    assert!(matches!(
        &docs.transport,
        crate::types::McpTransportInput::StreamableHttp {
            url,
            headers,
            bearer_token_env_var,
            scopes,
            oauth_resource,
            oauth_client_id,
        }
            if url == "https://example.test/mcp"
                && headers.get("x-test").map(String::as_str) == Some("yes")
                && bearer_token_env_var.as_deref() == Some("DOCS_MCP_TOKEN")
                && scopes == &vec!["docs.read".to_string()]
                && oauth_resource.as_deref() == Some("https://auth.example.test")
                && oauth_client_id.as_deref() == Some("psychevo")
    ));
}

#[test]
fn profile_mcp_servers_reject_inline_http_tokens_and_stdio_oauth() {
    let inline = crate::config::config_parse::parse_run_config(json!({
        "mcp_servers": {
            "docs": {
                "transport": "streamable_http",
                "url": "https://example.test/mcp",
                "headers": { "Authorization": "Bearer secret" }
            }
        }
    }))
    .expect_err("inline bearer");
    assert!(inline.to_string().contains("bearer_token_env_var"));

    let stdio = crate::config::config_parse::parse_run_config(json!({
        "mcp_servers": {
            "repo": {
                "transport": "stdio",
                "command": "node",
                "oauth": { "client_id": "bad" }
            }
        }
    }))
    .expect_err("stdio oauth");
    assert!(stdio.to_string().contains("only valid for streamable HTTP"));
}
