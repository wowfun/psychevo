use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use psychevo_ai::{
    AdapterCall, AdapterFuture, AdapterStream, LanguageAdapter, LanguageAdapterEvent,
    LanguageRequest, Outcome,
};

use crate::agents::catalog_surface::default_subagent_entrypoints;
use crate::agents::definition_policy::built_in_agent;
use crate::agents::{AgentBackendRef, AgentDefinition};
use crate::error::{Error, Result};
use crate::types::ExternalAgentDelegateRequest;

#[derive(Debug, Default)]
pub(super) struct FakeExternalAgentDelegate {
    pub(super) calls: Arc<Mutex<Vec<ExternalAgentDelegateRequest>>>,
    pub(super) failure: Option<String>,
}

impl crate::types::ExternalAgentDelegate for FakeExternalAgentDelegate {
    fn run(
        &self,
        request: ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, Result<crate::types::ExternalAgentDelegateResult>> {
        self.calls
            .lock()
            .expect("delegate calls lock poisoned")
            .push(request.clone());
        let failure = self.failure.clone();
        Box::pin(async move {
            if let Some(message) = failure {
                return Err(Error::Message(message));
            }
            Ok(crate::types::ExternalAgentDelegateResult {
                child_session_id: request.child_session_id,
                final_answer: "delegated final".to_string(),
                outcome: Outcome::Normal,
            })
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct AbortAwareExternalAgentDelegate {
    pub(super) started: Arc<tokio::sync::Notify>,
}

impl crate::types::ExternalAgentDelegate for AbortAwareExternalAgentDelegate {
    fn run(
        &self,
        request: ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, Result<crate::types::ExternalAgentDelegateResult>> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_waiters();
            let mut abort = request.abort.clone();
            abort.wait_for_abort().await;
            Ok(crate::types::ExternalAgentDelegateResult {
                child_session_id: request.child_session_id,
                final_answer: String::new(),
                outcome: Outcome::Aborted,
            })
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct AbortAwareProvider {
    pub(super) started: Arc<tokio::sync::Notify>,
}

impl LanguageAdapter for AbortAwareProvider {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_waiters();
            let mut abort = call.context.abort;
            abort.wait_for_abort().await;
            Ok(Box::pin(futures::stream::pending()) as AdapterStream<_>)
        })
    }
}

pub(super) fn backend_backed_agent(name: &str, backend: &str) -> AgentDefinition {
    let mut agent = built_in_agent(name, "Backend agent", "Delegates.", None);
    agent.backend = Some(AgentBackendRef {
        name: backend.to_string(),
    });
    agent.entrypoints = default_subagent_entrypoints();
    agent
}
