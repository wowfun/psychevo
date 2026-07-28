use std::collections::BTreeMap;

use crate::{
    ImageModel, LanguageModel, ModelProfile, Provider, ProviderError, RealtimeModel, SpeechModel,
    TranscriptionModel,
};

#[derive(Debug, Default)]
pub struct RegistryBuilder {
    providers: BTreeMap<String, Provider>,
}

impl RegistryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(mut self, provider: Provider) -> Result<Self, ProviderError> {
        let deployment_id = provider.deployment_config().deployment_id.clone();
        if self
            .providers
            .insert(deployment_id.clone(), provider)
            .is_some()
        {
            return Err(ProviderError::configuration(format!(
                "deployment `{deployment_id}` is already registered"
            )));
        }
        Ok(self)
    }

    pub fn build(self) -> Registry {
        Registry {
            providers: self.providers,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Registry {
    providers: BTreeMap<String, Provider>,
}

impl Registry {
    pub fn builder() -> RegistryBuilder {
        RegistryBuilder::new()
    }

    pub fn language_model(&self, target: &str) -> Result<LanguageModel, ProviderError> {
        let (provider, model) = self.resolve(target)?;
        provider.language_model(model)
    }

    pub fn language_model_with_profile(
        &self,
        target: &str,
        profile: ModelProfile,
    ) -> Result<LanguageModel, ProviderError> {
        self.language_model(target)
            .map(|model| model.with_profile(profile))
    }

    pub fn image_model(&self, target: &str) -> Result<ImageModel, ProviderError> {
        let (provider, model) = self.resolve(target)?;
        provider.image_model(model)
    }

    pub fn transcription_model(&self, target: &str) -> Result<TranscriptionModel, ProviderError> {
        let (provider, model) = self.resolve(target)?;
        provider.transcription_model(model)
    }

    pub fn speech_model(&self, target: &str) -> Result<SpeechModel, ProviderError> {
        let (provider, model) = self.resolve(target)?;
        provider.speech_model(model)
    }

    pub fn realtime_model(&self, target: &str) -> Result<RealtimeModel, ProviderError> {
        let (provider, model) = self.resolve(target)?;
        provider.realtime_model(model)
    }

    pub fn deployment(&self, deployment_id: &str) -> Option<&Provider> {
        self.providers.get(deployment_id)
    }

    fn resolve<'a>(&'a self, target: &'a str) -> Result<(&'a Provider, &'a str), ProviderError> {
        let Some((deployment_id, model_id)) = target.split_once('/') else {
            return Err(ProviderError::configuration(
                "registry target must use exact `deployment/model` syntax",
            ));
        };
        if deployment_id.is_empty() || model_id.is_empty() {
            return Err(ProviderError::configuration(
                "registry target requires non-empty deployment and model ids",
            ));
        }
        let provider = self.providers.get(deployment_id).ok_or_else(|| {
            ProviderError::configuration(format!("deployment `{deployment_id}` is not registered"))
        })?;
        Ok((provider, model_id))
    }
}
