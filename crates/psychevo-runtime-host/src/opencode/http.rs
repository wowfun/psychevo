use std::path::Path;
use std::time::Duration;

use reqwest::{Client, RequestBuilder, Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{RetryClass, RuntimeError, RuntimeErrorStage};

use super::types::{
    AgentInfo, FileDiffInfo, HealthResponse, MessageWithParts, PermissionRequest, PromptBody,
    QuestionRequest, SessionCreateBody, SessionInfo, StatusMap, TodoInfo,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub(crate) struct OpenCodeHttp {
    client: Client,
    base_url: Url,
}

impl std::fmt::Debug for OpenCodeHttp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenCodeHttp")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl OpenCodeHttp {
    pub(crate) fn new(client: Client, base_url: Url) -> Self {
        Self { client, base_url }
    }

    pub(crate) async fn health(&self) -> Result<HealthResponse, RuntimeError> {
        let url = self.url(&["global", "health"], None)?;
        self.json(self.client.get(url), RuntimeErrorStage::Handshake)
            .await
    }

    pub(crate) async fn event_stream(&self) -> Result<Response, RuntimeError> {
        let url = self.url(&["global", "event"], None)?;
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|error| {
                transport_error("failed to connect to OpenCode global events", error)
            })?;
        check_status(response, RuntimeErrorStage::Handshake).await
    }

    pub(crate) async fn sessions(&self, cwd: &Path) -> Result<Vec<SessionInfo>, RuntimeError> {
        let url = self.url(&["session"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::History)
            .await
    }

    pub(crate) async fn create_session(
        &self,
        cwd: &Path,
        body: &SessionCreateBody,
    ) -> Result<SessionInfo, RuntimeError> {
        let url = self.url(&["session"], Some(cwd))?;
        self.json(self.client.post(url).json(body), RuntimeErrorStage::Binding)
            .await
    }

    pub(crate) async fn session(
        &self,
        cwd: &Path,
        session_id: &str,
    ) -> Result<Option<SessionInfo>, RuntimeError> {
        let url = self.url(&["session", session_id], Some(cwd))?;
        self.optional_json(self.client.get(url), RuntimeErrorStage::History)
            .await
    }

    pub(crate) async fn statuses(&self, cwd: &Path) -> Result<StatusMap, RuntimeError> {
        let url = self.url(&["session", "status"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn messages(
        &self,
        cwd: &Path,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<(Vec<MessageWithParts>, Option<String>), RuntimeError> {
        let mut url = self.url(&["session", session_id, "message"], Some(cwd))?;
        if let Some(cursor) = cursor {
            url.query_pairs_mut()
                .append_pair("limit", "100")
                .append_pair("before", cursor);
        }
        let response = check_status(
            self.client
                .get(url)
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|error| transport_error("failed to read OpenCode history", error))?,
            RuntimeErrorStage::History,
        )
        .await?;
        let cursor = response
            .headers()
            .get("x-next-cursor")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let messages = response
            .json::<Vec<MessageWithParts>>()
            .await
            .map_err(|error| decode_error(RuntimeErrorStage::History, error))?;
        Ok((messages, cursor))
    }

    pub(crate) async fn message(
        &self,
        cwd: &Path,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageWithParts>, RuntimeError> {
        let url = self.url(&["session", session_id, "message", message_id], Some(cwd))?;
        self.optional_json(self.client.get(url), RuntimeErrorStage::History)
            .await
    }

    pub(crate) async fn children(
        &self,
        cwd: &Path,
        session_id: &str,
    ) -> Result<Vec<SessionInfo>, RuntimeError> {
        let url = self.url(&["session", session_id, "children"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn todos(
        &self,
        cwd: &Path,
        session_id: &str,
    ) -> Result<Vec<TodoInfo>, RuntimeError> {
        let url = self.url(&["session", session_id, "todo"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn diff(
        &self,
        cwd: &Path,
        session_id: &str,
        message_id: Option<&str>,
    ) -> Result<Vec<FileDiffInfo>, RuntimeError> {
        let mut url = self.url(&["session", session_id, "diff"], Some(cwd))?;
        if let Some(message_id) = message_id {
            url.query_pairs_mut().append_pair("messageID", message_id);
        }
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn permissions(
        &self,
        cwd: &Path,
    ) -> Result<Vec<PermissionRequest>, RuntimeError> {
        let url = self.url(&["permission"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn questions(&self, cwd: &Path) -> Result<Vec<QuestionRequest>, RuntimeError> {
        let url = self.url(&["question"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn agents(&self, cwd: &Path) -> Result<Vec<AgentInfo>, RuntimeError> {
        let url = self.url(&["agent"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn mcp_status(&self, cwd: &Path) -> Result<Value, RuntimeError> {
        let url = self.url(&["mcp"], Some(cwd))?;
        self.json(self.client.get(url), RuntimeErrorStage::Hydration)
            .await
    }

    pub(crate) async fn prompt_async(
        &self,
        cwd: &Path,
        session_id: &str,
        body: &PromptBody,
    ) -> Result<(), RuntimeError> {
        let url = self.url(&["session", session_id, "prompt_async"], Some(cwd))?;
        let response = self
            .client
            .post(url)
            .json(body)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|_| {
                RuntimeError::new(
                    "prompt_delivery_unknown",
                    RuntimeErrorStage::Prompt,
                    RetryClass::UnknownDelivery,
                    "OpenCode prompt delivery could not be confirmed; it was not resent",
                )
            })?;
        check_status(response, RuntimeErrorStage::Prompt).await?;
        Ok(())
    }

    pub(crate) async fn abort(&self, cwd: &Path, session_id: &str) -> Result<(), RuntimeError> {
        let url = self.url(&["session", session_id, "abort"], Some(cwd))?;
        self.json::<bool>(
            self.client.post(url).timeout(REQUEST_TIMEOUT),
            RuntimeErrorStage::Control,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn fork(
        &self,
        cwd: &Path,
        session_id: &str,
        argument: Option<&Value>,
    ) -> Result<SessionInfo, RuntimeError> {
        let url = self.url(&["session", session_id, "fork"], Some(cwd))?;
        self.json(
            self.client
                .post(url)
                .json(argument.unwrap_or(&Value::Object(Default::default()))),
            RuntimeErrorStage::History,
        )
        .await
    }

    pub(crate) async fn revert(
        &self,
        cwd: &Path,
        session_id: &str,
        argument: &Value,
    ) -> Result<SessionInfo, RuntimeError> {
        let url = self.url(&["session", session_id, "revert"], Some(cwd))?;
        self.json(
            self.client.post(url).json(argument),
            RuntimeErrorStage::History,
        )
        .await
    }

    pub(crate) async fn unrevert(
        &self,
        cwd: &Path,
        session_id: &str,
    ) -> Result<SessionInfo, RuntimeError> {
        let url = self.url(&["session", session_id, "unrevert"], Some(cwd))?;
        self.json(self.client.post(url), RuntimeErrorStage::History)
            .await
    }

    pub(crate) async fn rename(
        &self,
        cwd: &Path,
        session_id: &str,
        title: &str,
    ) -> Result<SessionInfo, RuntimeError> {
        let url = self.url(&["session", session_id], Some(cwd))?;
        self.json(
            self.client.patch(url).json(&json!({ "title": title })),
            RuntimeErrorStage::History,
        )
        .await
    }

    pub(crate) async fn archive(
        &self,
        cwd: &Path,
        session_id: &str,
        archived_at_ms: i64,
    ) -> Result<SessionInfo, RuntimeError> {
        let url = self.url(&["session", session_id], Some(cwd))?;
        self.json(
            self.client
                .patch(url)
                .json(&json!({ "time": { "archived": archived_at_ms } })),
            RuntimeErrorStage::History,
        )
        .await
    }

    pub(crate) async fn delete(&self, cwd: &Path, session_id: &str) -> Result<(), RuntimeError> {
        let url = self.url(&["session", session_id], Some(cwd))?;
        self.json::<bool>(self.client.delete(url), RuntimeErrorStage::History)
            .await?;
        Ok(())
    }

    pub(crate) async fn reply_permission(
        &self,
        cwd: &Path,
        request_id: &str,
        body: &Value,
    ) -> Result<bool, RuntimeError> {
        let url = self.url(&["permission", request_id, "reply"], Some(cwd))?;
        self.optional_success(
            self.client.post(url).json(body),
            RuntimeErrorStage::Interaction,
        )
        .await
    }

    pub(crate) async fn reply_question(
        &self,
        cwd: &Path,
        request_id: &str,
        body: &Value,
    ) -> Result<bool, RuntimeError> {
        let url = self.url(&["question", request_id, "reply"], Some(cwd))?;
        self.optional_success(
            self.client.post(url).json(body),
            RuntimeErrorStage::Interaction,
        )
        .await
    }

    pub(crate) async fn reject_question(
        &self,
        cwd: &Path,
        request_id: &str,
    ) -> Result<bool, RuntimeError> {
        let url = self.url(&["question", request_id, "reject"], Some(cwd))?;
        self.optional_success(self.client.post(url), RuntimeErrorStage::Interaction)
            .await
    }

    async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        stage: RuntimeErrorStage,
    ) -> Result<T, RuntimeError> {
        let response = request
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| transport_error("OpenCode HTTP request failed", error))?;
        check_status(response, stage)
            .await?
            .json::<T>()
            .await
            .map_err(|error| decode_error(stage, error))
    }

    async fn optional_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        stage: RuntimeErrorStage,
    ) -> Result<Option<T>, RuntimeError> {
        let response = request
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| transport_error("OpenCode HTTP request failed", error))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(
            check_status(response, stage)
                .await?
                .json::<T>()
                .await
                .map_err(|error| decode_error(stage, error))?,
        ))
    }

    async fn optional_success(
        &self,
        request: RequestBuilder,
        stage: RuntimeErrorStage,
    ) -> Result<bool, RuntimeError> {
        let response = request
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| transport_error("OpenCode interaction request failed", error))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        check_status(response, stage).await?;
        Ok(true)
    }

    fn url(&self, segments: &[&str], cwd: Option<&Path>) -> Result<Url, RuntimeError> {
        if segments.iter().any(|segment| {
            segment.is_empty()
                || segment.contains('/')
                || segment.contains('?')
                || segment.contains('#')
        }) {
            return Err(RuntimeError::new(
                "invalid_native_id",
                RuntimeErrorStage::Configuration,
                RetryClass::Never,
                "OpenCode request contained an invalid native path segment",
            ));
        }
        let mut url = self.base_url.clone();
        {
            let mut path = url.path_segments_mut().map_err(|_| {
                RuntimeError::new(
                    "invalid_server_url",
                    RuntimeErrorStage::Transport,
                    RetryClass::Never,
                    "OpenCode server URL cannot be used as a request base",
                )
            })?;
            path.clear();
            path.extend(segments.iter().copied());
        }
        if let Some(cwd) = cwd {
            url.query_pairs_mut()
                .append_pair("directory", &cwd.to_string_lossy());
        }
        Ok(url)
    }
}

async fn check_status(
    response: Response,
    stage: RuntimeErrorStage,
) -> Result<Response, RuntimeError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(RuntimeError::new(
            "authentication_failed",
            RuntimeErrorStage::Authentication,
            RetryClass::UserAction,
            "OpenCode rejected its process-scoped server credentials",
        ));
    }
    Err(RuntimeError::new(
        format!("http_{}", status.as_u16()),
        stage,
        if status.is_server_error() {
            RetryClass::SafeRetry
        } else {
            RetryClass::UserAction
        },
        format!("OpenCode request failed with HTTP {}", status.as_u16()),
    ))
}

fn transport_error(context: &str, error: reqwest::Error) -> RuntimeError {
    RuntimeError::new(
        "transport_error",
        RuntimeErrorStage::Transport,
        RetryClass::Reconnect,
        format!("{context}: {error}"),
    )
}

fn decode_error(stage: RuntimeErrorStage, error: reqwest::Error) -> RuntimeError {
    RuntimeError::new(
        "invalid_response",
        stage,
        RetryClass::Reconnect,
        format!("OpenCode returned an invalid response: {error}"),
    )
}
