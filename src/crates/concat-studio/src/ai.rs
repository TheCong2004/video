// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed native AI gateway actions. Provider credentials stay on the server.
use crate::workflows::{WorkflowClient, WorkflowError};
use serde::Deserialize;
use serde_json::{Value, json};

/// Actual provider outcome; these do not imply that edits have been approved/applied.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditPlanOutcome {
    /// A JSON proposal, still requiring strict native compilation and user approval.
    Plan,
    /// Explicit provider refusal.
    Refused,
    /// Provider exhausted output budget.
    Truncated,
    /// Selected model did not advertise strict structured-output support.
    Unsupported,
    /// Provider response could not be verified as an edit plan.
    Invalid,
}

/// Real reported usage, omitted if the provider supplies none.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct EditPlanUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Bounded provider receipt, not an executor result.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct EditPlanAnswer {
    pub outcome: EditPlanOutcome,
    pub model: String,
    pub plan: Option<Value>,
    pub finish_reason: Option<String>,
    pub usage: Option<EditPlanUsage>,
}

/// Display-only connection metadata: never copy API keys or OAuth tokens.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub active: bool,
    pub default_model: String,
}

/// A published model is not proof that a particular key can invoke it.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct Model {
    pub id: String,
    pub owner: String,
}

/// Successful generated text and the model that actually answered.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct Answer {
    pub text: String,
    pub model: String,
}

/// User-safe failures without upstream messages or credential-bearing payloads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiError {
    /// Input rejected before transmission.
    Input,
    /// Known gateway response or configuration error.
    Remote(WorkflowError),
    /// May have consumed quota. Never automatically resend.
    UnknownOutcome,
}

fn text(value: &Value, key: &str, max: usize, optional: bool) -> Result<String, WorkflowError> {
    let raw = match value.get(key) {
        None | Some(Value::Null) if optional => "",
        Some(Value::String(s)) => s,
        _ => return Err(WorkflowError::InvalidResponse),
    };
    if raw.len() > max || (!optional && raw.is_empty()) || raw.chars().any(char::is_control) {
        return Err(WorkflowError::InvalidResponse);
    }
    Ok(raw.to_owned())
}

fn segment(id: &str) -> Result<String, AiError> {
    if id.is_empty()
        || id.len() > 256
        || id == "."
        || id == ".."
        || id.chars().any(char::is_control)
    {
        return Err(AiError::Input);
    }
    let mut u = url::Url::parse("https://unused.invalid/").map_err(|_| AiError::Input)?;
    u.path_segments_mut().map_err(|_| AiError::Input)?.push(id);
    Ok(u.path().trim_start_matches('/').to_owned())
}

fn submitted(error: WorkflowError) -> AiError {
    match error {
        WorkflowError::Authentication
        | WorkflowError::Forbidden
        | WorkflowError::RateLimited
        | WorkflowError::Configuration => AiError::Remote(error),
        _ => AiError::UnknownOutcome,
    }
}

impl WorkflowClient {
    /// Read actual advertised tool capability; catalogue membership is not enough.
    pub fn ai_supports_tools(&self, model: &str) -> Result<bool, WorkflowError> {
        let catalogue = self.get("/api/v1/artcraft/ai/models")?;
        Ok(catalogue
            .get("data")
            .and_then(Value::as_array)
            .and_then(|models| {
                models
                    .iter()
                    .find(|entry| entry.get("id").and_then(Value::as_str) == Some(model))
            })
            .and_then(|entry| entry.get("supported_parameters"))
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("tools"))))
    }

    /// One bounded tool-calling turn through the existing AI provider service.
    /// No mutation happens here and uncertain dispatches are never retried.
    pub fn ai_agent_turn(
        &self,
        model: &str,
        messages: &[Value],
        tools: &[Value],
        max_tokens: u32,
        timeout: std::time::Duration,
    ) -> Result<Value, AiError> {
        if model.is_empty()
            || model.len() > 500
            || model.chars().any(char::is_control)
            || messages.is_empty()
            || messages.len() > 32
            || tools.len() > 16
            || tools.is_empty()
            || !(1..=4096).contains(&max_tokens)
            || timeout.is_zero()
            || timeout.as_secs() > 80
        {
            return Err(AiError::Input);
        }
        let body = json!({"model":model,"messages":messages,"tools":tools,"stream":false,"temperature":0,"maxTokens":max_tokens});
        if body.to_string().len() > 65536 {
            return Err(AiError::Input);
        }
        let result = self
            .write_with_timeout("POST", "/api/v1/artcraft/ai/chat", &body, timeout)
            .map_err(submitted)?;
        if result.to_string().len() > 70 * 1024 {
            return Err(AiError::UnknownOutcome);
        }
        text(&result, "model", 500, false).map_err(|_| AiError::UnknownOutcome)?;
        if !matches!(
            result.get("finishReason").and_then(Value::as_str),
            Some("stop" | "tool_calls" | "length" | "content_filter")
        ) {
            return Err(AiError::UnknownOutcome);
        }
        Ok(result)
    }
    /// Request one structured proposal through the existing authenticated gateway.
    /// Context must be scoped read-only timeline metadata, never paths/credentials.
    /// There is no automatic retry after dispatch; an uncertain response may consume quota.
    pub fn ai_edit_plan(
        &self,
        model: &str,
        prompt: &str,
        timeline_id: &str,
        base_revision: u64,
        context: &Value,
    ) -> Result<EditPlanAnswer, AiError> {
        if model.trim().is_empty()
            || model.len() > 500
            || model.chars().any(char::is_control)
            || prompt.trim().is_empty()
            || prompt.len() > 8192
            || prompt.contains('\0')
            || timeline_id.is_empty()
            || timeline_id.len() > 128
            || timeline_id.chars().any(char::is_control)
            || base_revision > 9_007_199_254_740_991
            || !context.is_object()
            || context.to_string().len() > 32768
        {
            return Err(AiError::Input);
        }
        let value = self
            .write_with_timeout(
                "POST",
                "/api/v1/artcraft/ai/edit-plan",
                &json!({
                    "model": model, "prompt": prompt, "timelineId": timeline_id,
                    "baseRevision": base_revision, "context": context,
                }),
                std::time::Duration::from_secs(80),
            )
            .map_err(submitted)?;
        if value.to_string().len() > 70 * 1024 {
            return Err(AiError::UnknownOutcome);
        }
        let receipt: EditPlanAnswer =
            serde_json::from_value(value).map_err(|_| AiError::UnknownOutcome)?;
        if receipt.model.is_empty()
            || receipt.model.len() > 500
            || receipt.model.chars().any(char::is_control)
            || receipt.finish_reason.as_ref().is_some_and(|r| {
                !["stop", "length", "tool_calls", "content_filter"].contains(&r.as_str())
            })
        {
            return Err(AiError::UnknownOutcome);
        }
        match (&receipt.outcome, &receipt.plan) {
            (EditPlanOutcome::Plan, Some(plan))
                if receipt.finish_reason.as_deref() == Some("stop")
                    && plan.get("version").and_then(Value::as_u64) == Some(1)
                    && plan.get("units").and_then(Value::as_str) == Some("seconds")
                    && plan.get("timelineId").and_then(Value::as_str) == Some(timeline_id)
                    && plan.get("baseRevision").and_then(Value::as_u64) == Some(base_revision) => {}
            (EditPlanOutcome::Plan, _) | (_, Some(_)) => return Err(AiError::UnknownOutcome),
            _ => {}
        }
        Ok(receipt)
    }
    /// Create one API-key connection. No key is retained locally or returned.
    pub fn ai_add_provider(
        &self,
        provider: &str,
        name: &str,
        key: &str,
        model: &str,
    ) -> Result<(), AiError> {
        if provider.is_empty()
            || provider.len() > 100
            || !provider
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            || name.trim().is_empty()
            || name.len() > 200
            || name.chars().any(char::is_control)
            || key.trim().is_empty()
            || key.len() > 8192
            || key.chars().any(char::is_control)
            || model.len() > 200
            || model.chars().any(char::is_control)
        {
            return Err(AiError::Input);
        }
        let result=self.post("/api/v1/artcraft/ai/providers", &json!({"provider":provider,"name":name,"apiKey":key,"defaultModel":model,"priority":1})).map_err(submitted)?;
        let connection = result.get("connection").ok_or(AiError::UnknownOutcome)?;
        text(connection, "id", 256, false).map_err(|_| AiError::UnknownOutcome)?;
        if connection.get("provider").and_then(Value::as_str) != Some(provider) {
            return Err(AiError::UnknownOutcome);
        }
        Ok(())
    }

    /// Enable/disable an existing connection. Server authorization still applies.
    pub fn ai_enable_provider(&self, id: &str, active: bool) -> Result<(), AiError> {
        let path = segment(id)?;
        let result = self
            .write(
                "PUT",
                &format!("/api/v1/artcraft/ai/providers/{path}"),
                &json!({"isActive":active}),
            )
            .map_err(submitted)?;
        let connection = result.get("connection").ok_or(AiError::UnknownOutcome)?;
        if connection.get("id").and_then(Value::as_str) != Some(id)
            || connection.get("isActive").and_then(Value::as_bool) != Some(active)
        {
            return Err(AiError::UnknownOutcome);
        }
        Ok(())
    }

    /// Permanently remove exactly the confirmed connection on the server.
    pub fn ai_delete_provider(&self, id: &str) -> Result<(), AiError> {
        let id = segment(id)?;
        let result = self
            .write(
                "DELETE",
                &format!("/api/v1/artcraft/ai/providers/{id}"),
                &json!({}),
            )
            .map_err(submitted)?;
        if result.get("message").and_then(Value::as_str) != Some("Connection deleted successfully")
        {
            return Err(AiError::UnknownOutcome);
        }
        Ok(())
    }
    /// Read administrator-visible connections; discard all credential fields.
    pub fn ai_providers(&self) -> Result<Vec<Provider>, WorkflowError> {
        parse_providers(&self.get("/api/v1/artcraft/ai/providers")?)
    }

    /// Read the configured gateway's model catalogue, without generating text.
    pub fn ai_models(&self) -> Result<Vec<Model>, WorkflowError> {
        parse_models(&self.get("/api/v1/artcraft/ai/models")?)
    }

    /// Tests the chosen connection, not a chat response. May consume provider quota.
    pub fn ai_test_connection(&self, id: &str, model: &str) -> Result<bool, AiError> {
        let id = segment(id)?;
        if model.len() > 500 || model.chars().any(char::is_control) {
            return Err(AiError::Input);
        }
        let body = if model.is_empty() {
            json!({})
        } else {
            json!({"validationModelId": model})
        };
        let value = self
            .post(&format!("/api/v1/artcraft/ai/providers/{id}/test"), &body)
            .map_err(submitted)?;
        value
            .get("valid")
            .and_then(Value::as_bool)
            .ok_or(AiError::UnknownOutcome)
    }

    /// One explicit bounded structured-text request for grounded editing plans.
    /// Uses the existing protected gateway; no automatic retry or mutation.
    /// Callers must validate the returned proposal against native source evidence.
    /// Dispatches the selected ID unchanged; `Answer::model` retains the actual
    /// provider model, which can differ for router aliases or provider prefixes.
    pub fn ai_structured(&self, model: &str, prompt: &str) -> Result<Answer, AiError> {
        const LIMIT: usize = 128 * 1024;
        if model.trim().is_empty()
            || model.len() > 500
            || model.chars().any(char::is_control)
            || prompt.trim().is_empty()
            || prompt.len() > LIMIT
            || prompt.contains('\0')
        {
            return Err(AiError::Input);
        }
        let value = self.write_with_timeout(
            "POST", "/api/v1/artcraft/ai/chat",
            &json!({"model":model,"messages":[{"role":"user","content":prompt}],"stream":false,"maxTokens":4096,"temperature":0}),
            std::time::Duration::from_secs(80),
        ).map_err(submitted)?;
        if value.to_string().len() > LIMIT
            || value.get("finishReason").and_then(Value::as_str) != Some("stop")
            || value
                .get("refusal")
                .is_some_and(|r| !r.is_null() && r.as_str() != Some(""))
        {
            return Err(AiError::UnknownOutcome);
        }
        let actual = text(&value, "model", 500, false).map_err(|_| AiError::UnknownOutcome)?;
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= LIMIT && !s.contains('\0'))
            .ok_or(AiError::UnknownOutcome)?;
        Ok(Answer {
            text: content.to_owned(),
            model: actual,
        })
    }

    /// One explicit routed chat request. Does not claim to pin a provider key.
    pub fn ai_ask(&self, model: &str, prompt: &str) -> Result<Answer, AiError> {
        if model.trim().is_empty()
            || model.len() > 500
            || model.chars().any(char::is_control)
            || prompt.trim().is_empty()
            || prompt.len() > 8192
            || prompt.contains('\0')
        {
            return Err(AiError::Input);
        }
        let value = self
            .post(
                "/api/v1/artcraft/ai/chat",
                &json!({
                    "model":model, "messages":[{"role":"user","content":prompt}],
                    "stream":false, "maxTokens":256, "temperature":0.4
                }),
            )
            .map_err(submitted)?;
        let answer = value
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= 65536 && !s.contains('\0'))
            .ok_or(AiError::UnknownOutcome)?;
        let model = text(&value, "model", 500, false).map_err(|_| AiError::UnknownOutcome)?;
        Ok(Answer {
            text: answer.to_owned(),
            model,
        })
    }
}

fn parse_providers(value: &Value) -> Result<Vec<Provider>, WorkflowError> {
    let list = value
        .get("connections")
        .and_then(Value::as_array)
        .filter(|v| v.len() <= 1000)
        .ok_or(WorkflowError::InvalidResponse)?;
    list.iter()
        .map(|v| {
            Ok(Provider {
                id: text(v, "id", 256, false)?,
                name: text(v, "name", 512, true)?,
                provider: text(v, "provider", 256, false)?,
                default_model: text(v, "defaultModel", 500, true)?,
                active: v
                    .get("isActive")
                    .and_then(Value::as_bool)
                    .ok_or(WorkflowError::InvalidResponse)?,
            })
        })
        .collect()
}

fn parse_models(value: &Value) -> Result<Vec<Model>, WorkflowError> {
    let list = value
        .get("data")
        .and_then(Value::as_array)
        .filter(|v| v.len() <= 5000)
        .ok_or(WorkflowError::InvalidResponse)?;
    let mut seen = std::collections::HashSet::new();
    list.iter()
        .map(|v| {
            let id = text(v, "id", 500, false)?;
            if !seen.insert(id.clone()) {
                return Err(WorkflowError::InvalidResponse);
            }
            Ok(Model {
                id,
                owner: text(v, "owned_by", 256, true)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    fn fixture(data: Value, status: u16) -> (WorkflowClient, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..n]);
                let s = String::from_utf8_lossy(&request);
                if let Some(end) = s.find("\r\n\r\n") {
                    let length = s[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let body = json!({"ok":true,"data":data}).to_string();
            write!(socket,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (
            WorkflowClient::new(&origin, Some("fixture-token".into())).unwrap(),
            worker,
        )
    }
    #[test]
    fn credentials_never_enter_provider_model() {
        let rows=parse_providers(&json!({"connections":[{"id":"a","provider":"groq","name":"main","isActive":true,"apiKey":"secret","accessToken":"secret","providerSpecificData":{"secret":"secret"}}]})).unwrap();
        assert!(!format!("{rows:?}").contains("secret"));
        assert!(parse_providers(&json!({"connections":[{"id":"x","provider":"groq"}]})).is_err());
    }
    #[test]
    fn catalogue_is_bounded_and_not_a_capability_test() {
        assert_eq!(
            parse_models(&json!({"data":[{"id":"groq/model","owned_by":"groq"}]})).unwrap()[0].id,
            "groq/model"
        );
        assert!(parse_models(&json!({"data":[{"id":"x"},{"id":"x"}]})).is_err());
        assert!(parse_models(&json!({"data":[{"id":"x\n"}]})).is_err());
    }
    #[test]
    fn chosen_connection_is_encoded_and_only_tested_once() {
        let (client, worker) = fixture(json!({"valid":false,"error":"secret"}), 200);
        assert_eq!(
            client.ai_test_connection("id/other?", "chosen-model"),
            Ok(false)
        );
        let req = worker.join().unwrap();
        assert!(req.starts_with("POST /api/v1/artcraft/ai/providers/id%2Fother%3F/test "));
        assert!(req.contains("validationModelId"));
        assert!(!req.contains("/ai/chat"));
    }
    #[test]
    fn chat_uses_actual_reply_not_echo_or_health() {
        let (client, worker) = fixture(json!({"content":"Xin chào!","model":"actual-model"}), 200);
        assert_eq!(
            client.ai_ask("chosen-model", "Hello").unwrap().text,
            "Xin chào!"
        );
        let req = worker.join().unwrap();
        assert!(req.starts_with("POST /api/v1/artcraft/ai/chat "));
        assert!(req.contains("\"stream\":false"));
        assert!(!req.contains("providerConnectionId"));
    }
    #[test]
    fn structured_chat_dispatches_exact_model_and_preserves_resolved_receipt() {
        let prompt = "grounded transcript ".repeat(1000);
        let (client, worker) = fixture(
            json!({"content":"{\"clips\":[]}","model":"resolved-provider-model","finishReason":"stop"}),
            200,
        );
        let answer = client.ai_structured("auto/best-coding", &prompt).unwrap();
        assert_eq!(answer.model, "resolved-provider-model");
        assert_eq!(answer.text, "{\"clips\":[]}");
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /api/v1/artcraft/ai/chat "));
        assert!(request.contains("Bearer fixture-token"));
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["maxTokens"], 4096);
        assert_eq!(body["model"], "auto/best-coding");
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["messages"][0]["content"], prompt);
    }
    #[test]
    fn structured_chat_rejects_invalid_model_truncation_and_oversized_content() {
        for data in [
            json!({"content":"{}","model":"","finishReason":"stop"}),
            json!({"content":"{}","finishReason":"stop"}),
            json!({"content":"{}","model":"exact","finishReason":"length"}),
            json!({"content":"{}","model":"exact"}),
            json!({"content":"x".repeat(128*1024+1),"model":"exact","finishReason":"stop"}),
        ] {
            let (client, worker) = fixture(data.clone(), 200);
            assert_eq!(
                client.ai_structured("exact", "plan"),
                Err(AiError::UnknownOutcome),
                "malformed structured response was misclassified: {data}"
            );
            worker.join().unwrap();
        }
        let client =
            WorkflowClient::new("http://127.0.0.1:1", Some("fixture-token".into())).unwrap();
        assert_eq!(
            client.ai_structured("exact", &"x".repeat(128 * 1024 + 1)),
            Err(AiError::Input)
        );
    }
    #[test]
    fn tool_turn_uses_existing_authenticated_chat_and_preserves_tool_receipt() {
        let receipt = json!({"model":"actual","finishReason":"tool_calls","content":"",
            "toolCalls":[{"id":"t1","type":"function","function":{"name":"read_project","arguments":"{}"}}],
            "usage":{"promptTokens":3,"completionTokens":4,"totalTokens":7}});
        let (client, worker) = fixture(receipt.clone(), 200);
        let messages = [json!({"role":"user","content":"inspect"})];
        let tools = [
            json!({"type":"function","function":{"name":"read_project","parameters":{"type":"object","properties":{}}}}),
        ];
        assert_eq!(
            client
                .ai_agent_turn(
                    "chosen",
                    &messages,
                    &tools,
                    128,
                    std::time::Duration::from_secs(5)
                )
                .unwrap(),
            receipt
        );
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /api/v1/artcraft/ai/chat "));
        assert!(request.contains("Bearer fixture-token"));
        assert!(request.contains("\"tools\""));
        assert!(request.contains("\"maxTokens\":128"));
        let (client, worker) = fixture(
            json!({"data":[{"id":"chosen","supported_parameters":["tools"]},{"id":"other"}]}),
            200,
        );
        assert!(client.ai_supports_tools("chosen").unwrap());
        assert!(
            worker
                .join()
                .unwrap()
                .starts_with("GET /api/v1/artcraft/ai/models ")
        );
        let (client, worker) = fixture(json!({"data":[{"id":"chosen"}]}), 200);
        assert!(!client.ai_supports_tools("chosen").unwrap());
        worker.join().unwrap();
    }
    #[test]
    fn structured_plan_preserves_scope_and_real_usage_without_applying() {
        let plan = json!({"version":1,"units":"seconds","timelineId":"tl","baseRevision":3,
            "operations":[{"op":"split","clipId":"clip","time":2.0,"as":"right"}]});
        let (client, worker) = fixture(
            json!({"outcome":"plan","model":"actual","plan":plan,
            "finishReason":"stop","usage":{"promptTokens":10,"completionTokens":20,"totalTokens":30}}),
            200,
        );
        let reply = client
            .ai_edit_plan("chosen", "split", "tl", 3, &json!({"clips":[]}))
            .unwrap();
        assert_eq!(reply.outcome, EditPlanOutcome::Plan);
        assert_eq!(reply.usage.unwrap().total_tokens, 30);
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /api/v1/artcraft/ai/edit-plan "));
        assert!(request.contains("\"baseRevision\":3"));
    }
    #[test]
    fn unsupported_or_truncated_plan_is_never_success_and_usage_is_not_invented() {
        let (client, worker) = fixture(json!({"outcome":"unsupported","model":"chosen"}), 200);
        let reply = client
            .ai_edit_plan("chosen", "split", "tl", 3, &json!({}))
            .unwrap();
        assert_eq!(reply.outcome, EditPlanOutcome::Unsupported);
        assert!(reply.usage.is_none() && reply.plan.is_none());
        worker.join().unwrap();
        let (client, worker) = fixture(
            json!({"outcome":"plan","model":"chosen","finishReason":"length","plan":{}}),
            200,
        );
        assert_eq!(
            client.ai_edit_plan("chosen", "split", "tl", 3, &json!({})),
            Err(AiError::UnknownOutcome)
        );
        worker.join().unwrap();
        let client = WorkflowClient::new("http://127.0.0.1:1", Some("fixture".into())).unwrap();
        assert_eq!(
            client.ai_edit_plan("chosen", "split", "tl", u64::MAX, &json!({})),
            Err(AiError::Input)
        );
    }
    #[test]
    fn unknown_or_error_response_never_retries_or_leaks_body() {
        let (client, worker) = fixture(json!({"secret":"credential"}), 200);
        assert_eq!(client.ai_ask("x", "Hello"), Err(AiError::UnknownOutcome));
        worker.join().unwrap();
        let (client, worker) = fixture(json!({"secret":"credential"}), 429);
        assert_eq!(
            client.ai_ask("x", "Hello"),
            Err(AiError::Remote(WorkflowError::RateLimited))
        );
        worker.join().unwrap();
    }
    #[test]
    fn input_validation_precedes_request() {
        let client = WorkflowClient::new("http://127.0.0.1:1", Some("fixture".into())).unwrap();
        assert_eq!(client.ai_ask("", "Hello"), Err(AiError::Input));
        assert_eq!(client.ai_test_connection("..", "x"), Err(AiError::Input));
        assert_eq!(
            client.ai_add_provider("groq", "main", "\n", ""),
            Err(AiError::Input)
        );
        assert_eq!(client.ai_delete_provider(".."), Err(AiError::Input));
    }
    #[test]
    fn incomplete_mutation_receipts_never_report_success() {
        let (client, worker) = fixture(json!({"connection":{"id":"new","provider":"other"}}), 201);
        assert_eq!(
            client.ai_add_provider("groq", "main", "fixture-key", ""),
            Err(AiError::UnknownOutcome)
        );
        worker.join().unwrap();
        let (client, worker) = fixture(json!({"connection":{"id":"other","isActive":true}}), 200);
        assert_eq!(
            client.ai_enable_provider("chosen", true),
            Err(AiError::UnknownOutcome)
        );
        worker.join().unwrap();
        let (client, worker) = fixture(json!({"error":"fixture-secret"}), 200);
        assert_eq!(
            client.ai_delete_provider("chosen"),
            Err(AiError::UnknownOutcome)
        );
        worker.join().unwrap();
    }
    #[test]
    fn provider_mutations_use_scoped_routes_and_preserve_credentials_on_toggle() {
        let (client, worker) = fixture(json!({"connection":{"id":"new","provider":"groq"}}), 201);
        client
            .ai_add_provider("groq", "main", "fixture-key", "model")
            .unwrap();
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /api/v1/artcraft/ai/providers "));
        assert!(request.contains("\"apiKey\":\"fixture-key\""));
        let (client, worker) = fixture(json!({"connection":{"id":"a/b","isActive":false}}), 200);
        client.ai_enable_provider("a/b", false).unwrap();
        let request = worker.join().unwrap();
        assert!(request.starts_with("PUT /api/v1/artcraft/ai/providers/a%2Fb "));
        assert!(request.ends_with("{\"isActive\":false}"));
        let (client, worker) = fixture(json!({"message":"Connection deleted successfully"}), 200);
        client.ai_delete_provider("a/b").unwrap();
        assert!(
            worker
                .join()
                .unwrap()
                .starts_with("DELETE /api/v1/artcraft/ai/providers/a%2Fb ")
        );
    }
}
