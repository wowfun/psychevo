# psychevo-ai

`psychevo-ai` is Psychevo's provider-neutral Rust SDK. It can be used directly
as a local path dependency and supports custom providers without requiring
`async_trait`.

```toml
[dependencies]
psychevo-ai = { path = "../psychevo/crates/psychevo-ai" }
```

For only the provider-neutral core and custom/fake Adapters:

```toml
[dependencies]
psychevo-ai = {
    path = "../psychevo/crates/psychevo-ai",
    default-features = false,
}
```

Bind an explicit deployment before invoking a model:

```rust,no_run
use psychevo_ai::{
    DeploymentConfig, LanguageRequest, Message, OpenAi, SecretValue,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let openai = OpenAi::builder(DeploymentConfig::new(
    "openai",
    "openai",
    "https://api.openai.com/v1",
))
.with_api_key(SecretValue::new("secret"))
.build()?;

let model = openai.responses("gpt-5")?;
let output = model
    .generate(LanguageRequest {
        messages: vec![Message::user("Hello")],
        ..LanguageRequest::default()
    })
    .await?;
# Ok(())
# }
```

Custom integrations implement one or more capability-specific Adapter traits
(`LanguageAdapter`, `ImageAdapter`, `TranscriptionAdapter`, `SpeechAdapter`, or
`RealtimeAdapter`) and attach them to a `Provider`:

```rust,no_run
use psychevo_ai::{DeploymentConfig, Provider};

# fn build(my_adapter: impl psychevo_ai::LanguageAdapter)
#   -> Result<Provider, psychevo_ai::ProviderError> {
Provider::builder(DeploymentConfig::new(
    "local",
    "my_provider",
    "http://127.0.0.1:8080",
))
.language_adapter(my_adapter)
.build()
# }
```

See the runnable
[`examples/custom_provider.rs`](examples/custom_provider.rs) example for a full
streaming Adapter.

Language `Generation` retains normalized events in an unbounded queue so a
Provider cannot deadlock when a caller temporarily stops polling. Keep
consuming the stream, call `abort`, or drop the handle; retaining a live
generation indefinitely without polling can grow memory without bound.

`Registry` freezes multiple explicit deployments and resolves exact
`deployment/model` identifiers. The SDK does not discover environment
variables, model catalogs, aliases, fallbacks, or product configuration.

Default features enable the `openai`, `anthropic`, and `xiaomi` built-ins.
Provider-neutral types, custom Adapter traits, Registry, and deterministic fake
Adapters remain available with `default-features = false`.
