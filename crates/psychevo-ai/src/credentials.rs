use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{AdapterFuture, AdapterResult, ErrorKind, ErrorPhase, ProviderError};

#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(Arc<str>);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Arc::<str>::from(value.into()))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialSlot(pub String);

impl CredentialSlot {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialRef(pub String);

impl CredentialRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialBindings(pub BTreeMap<CredentialSlot, CredentialRef>);

impl CredentialBindings {
    pub fn bind(mut self, slot: impl Into<String>, credential_ref: impl Into<String>) -> Self {
        self.0.insert(
            CredentialSlot::new(slot),
            CredentialRef::new(credential_ref),
        );
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Default)]
pub struct CredentialSnapshot {
    values: BTreeMap<CredentialSlot, SecretValue>,
}

impl fmt::Debug for CredentialSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialSnapshot")
            .field("slots", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CredentialSnapshot {
    pub fn new(values: BTreeMap<CredentialSlot, SecretValue>) -> Self {
        Self { values }
    }

    pub fn get(&self, slot: &str) -> Option<&SecretValue> {
        self.values.get(&CredentialSlot::new(slot))
    }

    pub fn require(&self, slot: &str) -> AdapterResult<&SecretValue> {
        self.get(slot).ok_or_else(|| {
            ProviderError::new(
                ErrorKind::Authentication,
                ErrorPhase::Credentials,
                format!("credential slot `{slot}` was not resolved"),
            )
        })
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct CredentialRequest {
    pub deployment_id: String,
    pub provider_family: String,
    pub bindings: CredentialBindings,
}

pub trait CredentialResolver: Send + Sync + 'static {
    fn resolve<'a>(&'a self, request: CredentialRequest) -> AdapterFuture<'a, CredentialSnapshot>;
}

#[derive(Debug, Default)]
pub struct EmptyCredentialResolver;

impl CredentialResolver for EmptyCredentialResolver {
    fn resolve<'a>(&'a self, request: CredentialRequest) -> AdapterFuture<'a, CredentialSnapshot> {
        Box::pin(async move {
            if request.bindings.is_empty() {
                Ok(CredentialSnapshot::default())
            } else {
                Err(ProviderError::new(
                    ErrorKind::Authentication,
                    ErrorPhase::Credentials,
                    format!(
                        "deployment `{}` configured credentials without a resolver",
                        request.deployment_id
                    ),
                ))
            }
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct StaticCredentialResolver {
    by_ref: BTreeMap<CredentialRef, SecretValue>,
}

impl StaticCredentialResolver {
    pub fn new(by_ref: BTreeMap<CredentialRef, SecretValue>) -> Self {
        Self { by_ref }
    }

    pub fn single(credential_ref: impl Into<String>, value: SecretValue) -> Self {
        Self::new(BTreeMap::from([(
            CredentialRef::new(credential_ref),
            value,
        )]))
    }
}

impl CredentialResolver for StaticCredentialResolver {
    fn resolve<'a>(&'a self, request: CredentialRequest) -> AdapterFuture<'a, CredentialSnapshot> {
        Box::pin(async move {
            let mut values = BTreeMap::new();
            for (slot, credential_ref) in request.bindings.0 {
                let value = self.by_ref.get(&credential_ref).cloned().ok_or_else(|| {
                    ProviderError::new(
                        ErrorKind::Authentication,
                        ErrorPhase::Credentials,
                        format!(
                            "credential reference `{}` for slot `{}` was not resolved",
                            credential_ref.0, slot.0
                        ),
                    )
                })?;
                if value.is_empty() {
                    return Err(ProviderError::new(
                        ErrorKind::Authentication,
                        ErrorPhase::Credentials,
                        format!(
                            "credential reference `{}` for slot `{}` resolved to an empty secret",
                            credential_ref.0, slot.0
                        ),
                    ));
                }
                values.insert(slot, value);
            }
            Ok(CredentialSnapshot::new(values))
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct EnvironmentCredentialResolver {
    snapshot: BTreeMap<CredentialRef, Result<SecretValue, String>>,
}

impl EnvironmentCredentialResolver {
    pub fn capture(references: impl IntoIterator<Item = (CredentialRef, String)>) -> Self {
        let snapshot = references
            .into_iter()
            .map(|(credential_ref, environment_name)| {
                let value = capture_environment_value(&environment_name);
                (credential_ref, value)
            })
            .collect();
        Self { snapshot }
    }
}

impl CredentialResolver for EnvironmentCredentialResolver {
    fn resolve<'a>(&'a self, request: CredentialRequest) -> AdapterFuture<'a, CredentialSnapshot> {
        Box::pin(async move {
            let mut values = BTreeMap::new();
            for (slot, credential_ref) in request.bindings.0 {
                let value = self
                    .snapshot
                    .get(&credential_ref)
                    .ok_or_else(|| {
                        ProviderError::new(
                            ErrorKind::Authentication,
                            ErrorPhase::Credentials,
                            format!(
                                "environment snapshot does not contain credential reference `{}`",
                                credential_ref.0
                            ),
                        )
                    })?
                    .clone()
                    .map_err(|message| {
                        ProviderError::new(
                            ErrorKind::Authentication,
                            ErrorPhase::Credentials,
                            message,
                        )
                    })?;
                values.insert(slot, value);
            }
            Ok(CredentialSnapshot::new(values))
        })
    }
}

fn capture_environment_value(name: &str) -> Result<SecretValue, String> {
    let Some(value) = std::env::var_os(name) else {
        return Err(format!("environment variable `{name}` is missing"));
    };
    os_string_to_secret(name, value)
}

fn os_string_to_secret(name: &str, value: OsString) -> Result<SecretValue, String> {
    let value = value
        .into_string()
        .map_err(|_| format!("environment variable `{name}` is not valid UTF-8"))?;
    if value.is_empty() {
        return Err(format!("environment variable `{name}` is empty"));
    }
    Ok(SecretValue::new(value))
}
