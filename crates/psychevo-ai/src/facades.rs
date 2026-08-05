use std::sync::Arc;

#[cfg(feature = "anthropic")]
use crate::{AnthropicAuth, AnthropicMessagesAdapter};
use crate::{
    CredentialBindings, CredentialRef, DeploymentConfig, EnvironmentCredentialResolver, Provider,
    ProviderError, ProviderRuntime, SecretValue, StaticCredentialResolver,
};
#[cfg(feature = "openai")]
use crate::{ImageModel, OpenAiChatAdapter, OpenAiImageAdapter, OpenAiResponsesAdapter};
#[cfg(feature = "xiaomi")]
use crate::{SpeechModel, TranscriptionModel, XiaomiSpeechAdapter, XiaomiTranscriptionAdapter};

#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
const API_KEY_SLOT: &str = "api_key";
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
const EXPLICIT_API_KEY_REF: &str = "explicit:api_key";
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
const ENV_API_KEY_REF: &str = "environment:api_key";
#[cfg(feature = "anthropic")]
const EXPLICIT_BEARER_REF: &str = "explicit:bearer_token";
#[cfg(feature = "anthropic")]
const ENV_BEARER_REF: &str = "environment:bearer_token";

#[cfg(feature = "openai")]
#[derive(Debug, Clone)]
pub struct OpenAi {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
}

#[cfg(feature = "openai")]
impl OpenAi {
    pub fn builder(mut config: DeploymentConfig) -> OpenAiBuilder {
        if config.default_language_protocol == config.provider_family {
            config.default_language_protocol = "openai_responses".to_string();
        }
        OpenAiBuilder {
            config,
            runtime: ProviderRuntime::default(),
        }
    }

    pub fn chat(&self, model_id: impl Into<String>) -> Result<crate::LanguageModel, ProviderError> {
        self.protocol_provider("openai_chat")?
            .language_model(model_id)
    }

    pub fn responses(
        &self,
        model_id: impl Into<String>,
    ) -> Result<crate::LanguageModel, ProviderError> {
        self.protocol_provider("openai_responses")?
            .language_model(model_id)
    }

    pub fn image(&self, model_id: impl Into<String>) -> Result<ImageModel, ProviderError> {
        Provider::builder(self.config.clone())
            .runtime(self.runtime.clone())
            .image_adapter(OpenAiImageAdapter)
            .build()?
            .image_model(model_id)
    }

    pub fn provider(&self) -> Result<Provider, ProviderError> {
        let adapter: Arc<dyn crate::LanguageAdapter> =
            match self.config.default_language_protocol.as_str() {
                "openai_chat" => Arc::new(OpenAiChatAdapter),
                "openai_responses" => Arc::new(OpenAiResponsesAdapter),
                protocol => {
                    return Err(ProviderError::configuration(format!(
                        "unsupported OpenAI default language protocol `{protocol}`"
                    )));
                }
            };
        Provider::builder(self.config.clone())
            .runtime(self.runtime.clone())
            .language_adapter_arc(adapter)
            .image_adapter(OpenAiImageAdapter)
            .build()
    }

    fn protocol_provider(&self, protocol: &str) -> Result<Provider, ProviderError> {
        let config = self.config.clone().with_default_language_protocol(protocol);
        let builder = Provider::builder(config).runtime(self.runtime.clone());
        match protocol {
            "openai_chat" => builder.language_adapter(OpenAiChatAdapter).build(),
            "openai_responses" => builder.language_adapter(OpenAiResponsesAdapter).build(),
            _ => unreachable!("validated protocol"),
        }
    }
}

#[cfg(feature = "openai")]
#[derive(Debug, Clone)]
pub struct OpenAiBuilder {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
}

#[cfg(feature = "openai")]
impl OpenAiBuilder {
    pub fn with_api_key(mut self, secret: SecretValue) -> Self {
        self.config.credentials =
            CredentialBindings::default().bind(API_KEY_SLOT, EXPLICIT_API_KEY_REF);
        self.runtime =
            self.runtime
                .with_credential_resolver(Arc::new(StaticCredentialResolver::single(
                    EXPLICIT_API_KEY_REF,
                    secret,
                )));
        self
    }

    pub fn from_env_snapshot(mut self, environment_name: impl Into<String>) -> Self {
        self.config.credentials = CredentialBindings::default().bind(API_KEY_SLOT, ENV_API_KEY_REF);
        self.runtime = self.runtime.with_credential_resolver(Arc::new(
            EnvironmentCredentialResolver::capture([(
                CredentialRef::new(ENV_API_KEY_REF),
                environment_name.into(),
            )]),
        ));
        self
    }

    pub fn runtime(mut self, runtime: ProviderRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.runtime = self.runtime.with_client(client);
        self
    }

    pub fn credential_resolver(mut self, resolver: Arc<dyn crate::CredentialResolver>) -> Self {
        self.runtime = self.runtime.with_credential_resolver(resolver);
        self
    }

    pub fn build(self) -> Result<OpenAi, ProviderError> {
        let facade = OpenAi {
            config: self.config,
            runtime: self.runtime,
        };
        facade.provider()?;
        Ok(facade)
    }
}

#[cfg(feature = "xiaomi")]
#[derive(Debug, Clone)]
pub struct Xiaomi {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
}

#[cfg(feature = "xiaomi")]
impl Xiaomi {
    pub fn builder(config: DeploymentConfig) -> XiaomiBuilder {
        XiaomiBuilder {
            config,
            runtime: ProviderRuntime::default(),
        }
    }

    pub fn transcription(
        &self,
        model_id: impl Into<String>,
    ) -> Result<TranscriptionModel, ProviderError> {
        self.provider()?.transcription_model(model_id)
    }

    pub fn speech(&self, model_id: impl Into<String>) -> Result<SpeechModel, ProviderError> {
        self.provider()?.speech_model(model_id)
    }

    pub fn provider(&self) -> Result<Provider, ProviderError> {
        Provider::builder(self.config.clone())
            .runtime(self.runtime.clone())
            .transcription_adapter(XiaomiTranscriptionAdapter)
            .speech_adapter(XiaomiSpeechAdapter)
            .build()
    }
}

#[cfg(feature = "xiaomi")]
#[derive(Debug, Clone)]
pub struct XiaomiBuilder {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
}

#[cfg(feature = "xiaomi")]
impl XiaomiBuilder {
    pub fn with_api_key(mut self, secret: SecretValue) -> Self {
        self.config.credentials =
            CredentialBindings::default().bind(API_KEY_SLOT, EXPLICIT_API_KEY_REF);
        self.runtime =
            self.runtime
                .with_credential_resolver(Arc::new(StaticCredentialResolver::single(
                    EXPLICIT_API_KEY_REF,
                    secret,
                )));
        self
    }

    pub fn from_env_snapshot(mut self, environment_name: impl Into<String>) -> Self {
        self.config.credentials = CredentialBindings::default().bind(API_KEY_SLOT, ENV_API_KEY_REF);
        self.runtime = self.runtime.with_credential_resolver(Arc::new(
            EnvironmentCredentialResolver::capture([(
                CredentialRef::new(ENV_API_KEY_REF),
                environment_name.into(),
            )]),
        ));
        self
    }

    pub fn runtime(mut self, runtime: ProviderRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    pub fn build(self) -> Result<Xiaomi, ProviderError> {
        let facade = Xiaomi {
            config: self.config,
            runtime: self.runtime,
        };
        facade.provider()?;
        Ok(facade)
    }
}

#[cfg(feature = "anthropic")]
#[derive(Debug, Clone)]
pub struct Anthropic {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
    auth: AnthropicAuth,
}

#[cfg(feature = "anthropic")]
impl Anthropic {
    pub fn builder(mut config: DeploymentConfig) -> AnthropicBuilder {
        if config.default_language_protocol == config.provider_family {
            config.default_language_protocol = "anthropic_messages".to_string();
        }
        AnthropicBuilder {
            config,
            runtime: ProviderRuntime::default(),
            auth: AnthropicAuth::ApiKey,
        }
    }

    pub fn messages(
        &self,
        model_id: impl Into<String>,
    ) -> Result<crate::LanguageModel, ProviderError> {
        self.provider()?.language_model(model_id)
    }

    pub fn provider(&self) -> Result<Provider, ProviderError> {
        if self.config.default_language_protocol != "anthropic_messages" {
            return Err(ProviderError::configuration(format!(
                "unsupported Anthropic default language protocol `{}`",
                self.config.default_language_protocol
            )));
        }
        Provider::builder(self.config.clone())
            .runtime(self.runtime.clone())
            .language_adapter(AnthropicMessagesAdapter::new(self.auth))
            .build()
    }
}

#[cfg(feature = "anthropic")]
#[derive(Debug, Clone)]
pub struct AnthropicBuilder {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
    auth: AnthropicAuth,
}

#[cfg(feature = "anthropic")]
impl AnthropicBuilder {
    pub fn auth(mut self, auth: AnthropicAuth) -> Self {
        let previous_slot = match self.auth {
            AnthropicAuth::ApiKey => API_KEY_SLOT,
            AnthropicAuth::Bearer => "bearer_token",
        };
        let next_slot = match auth {
            AnthropicAuth::ApiKey => API_KEY_SLOT,
            AnthropicAuth::Bearer => "bearer_token",
        };
        if previous_slot != next_slot
            && let Some(credential_ref) = self
                .config
                .credentials
                .0
                .remove(&crate::CredentialSlot::new(previous_slot))
        {
            self.config
                .credentials
                .0
                .insert(crate::CredentialSlot::new(next_slot), credential_ref);
        }
        self.auth = auth;
        self
    }

    pub fn with_api_key(mut self, secret: SecretValue) -> Self {
        let (slot, credential_ref) = match self.auth {
            AnthropicAuth::ApiKey => (API_KEY_SLOT, EXPLICIT_API_KEY_REF),
            AnthropicAuth::Bearer => ("bearer_token", EXPLICIT_BEARER_REF),
        };
        self.config.credentials = CredentialBindings::default().bind(slot, credential_ref);
        self.runtime =
            self.runtime
                .with_credential_resolver(Arc::new(StaticCredentialResolver::single(
                    credential_ref,
                    secret,
                )));
        self
    }

    pub fn from_env_snapshot(mut self, environment_name: impl Into<String>) -> Self {
        let (slot, credential_ref) = match self.auth {
            AnthropicAuth::ApiKey => (API_KEY_SLOT, ENV_API_KEY_REF),
            AnthropicAuth::Bearer => ("bearer_token", ENV_BEARER_REF),
        };
        self.config.credentials = CredentialBindings::default().bind(slot, credential_ref);
        self.runtime = self.runtime.with_credential_resolver(Arc::new(
            EnvironmentCredentialResolver::capture([(
                CredentialRef::new(credential_ref),
                environment_name.into(),
            )]),
        ));
        self
    }

    pub fn runtime(mut self, runtime: ProviderRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    pub fn build(self) -> Result<Anthropic, ProviderError> {
        let facade = Anthropic {
            config: self.config,
            runtime: self.runtime,
            auth: self.auth,
        };
        facade.provider()?;
        Ok(facade)
    }
}

#[cfg(all(test, feature = "anthropic"))]
mod tests {
    use super::Anthropic;
    use crate::{AnthropicAuth, DeploymentConfig, SecretValue};

    fn config() -> DeploymentConfig {
        DeploymentConfig::new("anthropic", "anthropic", "https://api.anthropic.com")
    }

    #[test]
    fn anthropic_auth_selection_is_order_independent_for_explicit_credentials() {
        let auth_then_secret = Anthropic::builder(config())
            .auth(AnthropicAuth::Bearer)
            .with_api_key(SecretValue::new("token"))
            .build()
            .expect("auth then secret")
            .provider()
            .expect("provider");
        let secret_then_auth = Anthropic::builder(config())
            .with_api_key(SecretValue::new("token"))
            .auth(AnthropicAuth::Bearer)
            .build()
            .expect("secret then auth")
            .provider()
            .expect("provider");

        for provider in [auth_then_secret, secret_then_auth] {
            let bindings = &provider.deployment_config().credentials.0;
            assert!(bindings.contains_key(&crate::CredentialSlot::new("bearer_token")));
            assert!(!bindings.contains_key(&crate::CredentialSlot::new("api_key")));
        }
    }
}
