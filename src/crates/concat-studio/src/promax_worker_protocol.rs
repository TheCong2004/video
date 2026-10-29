//! Strict existing Promax artcraft_worker wire boundary.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerIdentity {
    pub client_id: String,
    pub device_id: String,
    pub tenant_id: String,
    pub user_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub envelope_id: String,
    pub protocol_version: String,
    pub channel: String,
    pub command_id: String,
    pub task_id: String,
    pub attempt_id: u64,
    pub target_client_id: String,
    pub tenant_id: String,
    pub device_id: String,
    pub fencing_token: u64,
    pub session_generation: u64,
    pub connection_generation: u64,
    pub deadline: String,
    pub capability: String,
    pub idempotency_key: String,
    pub payload: DeliveryPayload,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeliveryPayload {
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: Value,
}

pub(crate) fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}
impl Envelope {
    pub fn validate(&self, identity: &WorkerIdentity, generation: u64) -> Result<(), String> {
        let c = &self.payload.command;
        let same = |key: &str, expected: &str| c.get(key).and_then(Value::as_str) == Some(expected);
        let input = c.get("inputPayload").ok_or("WORKER_COMMAND_INVALID")?;
        let stage = c
            .get("stageId")
            .and_then(Value::as_str)
            .ok_or("WORKER_COMMAND_INVALID")?;
        if generation == 0
            || self.protocol_version != "1.0"
            || self.channel != "artcraft"
            || self.target_client_id != identity.client_id
            || self.device_id != identity.device_id
            || self.tenant_id != identity.tenant_id
            || self.connection_generation != generation
            || self.session_generation != generation
            || self.payload.message_type != "artcraft.workflow.step"
            || !same("commandId", &self.command_id)
            || !same("runId", &self.task_id)
            || c.get("attemptId").and_then(Value::as_u64) != Some(self.attempt_id)
            || c.get("fencingToken").and_then(Value::as_u64) != Some(self.fencing_token)
            || c.get("idempotencyKey").and_then(Value::as_str)
                != Some(self.idempotency_key.as_str())
            || input.get("_tenantId").and_then(Value::as_str) != Some(identity.tenant_id.as_str())
            || input.get("_ownerId").and_then(Value::as_str) != Some(identity.user_id.as_str())
            || input.get("_appId").and_then(Value::as_str) != Some("artcraft")
            || input.get("nodeType").and_then(Value::as_str) != Some(stage)
            || self.capability != format!("artcraft.workflow.{stage}")
        {
            return Err("WORKER_ENVELOPE_MISMATCH".into());
        }
        for id in [
            &self.envelope_id,
            &self.command_id,
            &self.task_id,
            &self.idempotency_key,
        ] {
            if !safe_id(id) {
                return Err("WORKER_ENVELOPE_INVALID".into());
            }
        }
        if !safe_id(stage)
            || !c.get("stepId").and_then(Value::as_str).is_some_and(safe_id)
            || self.attempt_id == 0
            || self.fencing_token == 0
        {
            return Err("WORKER_ENVELOPE_INVALID".into());
        }
        let now = chrono::DateTime::from_timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "CLOCK_UNAVAILABLE")?
                .as_secs() as i64,
            0,
        )
        .ok_or("CLOCK_UNAVAILABLE")?;
        if chrono::DateTime::parse_from_rfc3339(&self.deadline)
            .map_or(true, |deadline| deadline <= now)
        {
            return Err("DELIVERY_EXPIRED".into());
        }
        Ok(())
    }
    pub fn ack(
        &self,
        identity: &WorkerIdentity,
        generation: u64,
        accepted: bool,
        reason: &str,
    ) -> Value {
        json!({"type":"delivery.ack","ack":{"commandId":self.command_id,"taskId":self.task_id,"clientId":identity.client_id,"connectionGeneration":generation,"timestamp":timestamp(),"accepted":accepted,"rejectionReason":reason}})
    }
    pub fn terminal(&self, status: &str, receipt: Value) -> Value {
        let mut result = json!({"commandId":self.command_id,"runId":self.task_id,"stepId":self.payload.command["stepId"],"attemptId":self.attempt_id,"fencingToken":self.fencing_token,"status":status});
        if status == "completed" {
            result["output"] = receipt.get("output").cloned().unwrap_or_else(|| json!({}));
            if let Some(checkpoint) = receipt.get("checkpoint") {
                result["checkpoint"] = checkpoint.clone();
            }
        } else if status == "cancelled" {
            result["errorCode"] = json!("WORKER_CANCELLED_CONFIRMED");
            result["error"] = json!("Local stage cancellation was confirmed.");
        } else {
            result["errorCode"] = json!("NATIVE_STAGE_FAILED");
            result["error"] = json!("Local stage failed.");
        }
        result
    }
    pub fn frame(&self, identity: &WorkerIdentity, generation: u64, payload: Value) -> Value {
        let status = if payload["status"] == "completed" {
            "success"
        } else if payload["status"] == "cancelled" {
            "cancelled"
        } else {
            "failed"
        };
        json!({"type":"command.result","channel":"artcraft","attemptId":self.attempt_id,"connectionGeneration":generation,"result":{"commandId":self.command_id,"clientId":identity.client_id,"taskId":self.task_id,"status":status,"kind":"artcraft.workflow.step.result","payload":payload}})
    }
}
pub(crate) fn timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    chrono::DateTime::from_timestamp(seconds as i64, 0)
        .map_or_else(String::new, |time| time.to_rfc3339())
}
pub(crate) fn cancellation_matches(envelope: &Envelope, frame: &Value) -> bool {
    frame.get("type").and_then(Value::as_str) == Some("command.cancel")
        && frame["commandId"] == envelope.command_id
        && frame["taskId"] == envelope.task_id
}

#[cfg(test)]
pub(crate) fn fixture() -> (WorkerIdentity, Envelope) {
    let identity = WorkerIdentity {
        client_id: "concat-production-device".into(),
        device_id: "device".into(),
        tenant_id: "tenant".into(),
        user_id: "owner".into(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let deadline = chrono::DateTime::from_timestamp(now as i64 + 60, 0)
        .unwrap()
        .to_rfc3339();
    let envelope=serde_json::from_value(json!({"envelopeId":"env-1","protocolVersion":"1.0","channel":"artcraft","commandId":"cmd-1","taskId":"run-1","attemptId":1,"targetClientId":identity.client_id,"tenantId":"tenant","deviceId":"device","fencingToken":1,"sessionGeneration":2,"connectionGeneration":2,"deadline":deadline,"capability":"artcraft.workflow.stage.grok_image_edit","idempotencyKey":"run-1:stage-1:1","payload":{"type":"artcraft.workflow.step","command":{"commandId":"cmd-1","runId":"run-1","stepId":"stage-1","stageId":"stage.grok_image_edit","attemptId":1,"fencingToken":1,"idempotencyKey":"run-1:stage-1:1","inputPayload":{"_appId":"artcraft","_tenantId":"tenant","_ownerId":"owner","nodeType":"stage.grok_image_edit"}}}})).unwrap();
    (identity, envelope)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_all_authenticated_scope_and_command_correlations() {
        let (identity, envelope) = fixture();
        assert!(envelope.validate(&identity, 2).is_ok());
        for field in [
            "tenantId",
            "deviceId",
            "targetClientId",
            "protocolVersion",
            "channel",
        ] {
            let mut value = serde_json::to_value(&envelope).unwrap();
            value[field] = json!("wrong");
            assert!(
                serde_json::from_value::<Envelope>(value)
                    .unwrap()
                    .validate(&identity, 2)
                    .is_err()
            );
        }
        for field in ["commandId", "runId", "stepId", "stageId", "idempotencyKey"] {
            let mut value = serde_json::to_value(&envelope).unwrap();
            value["payload"]["command"][field] = json!("wrong");
            // stepId has no outer counterpart, but must remain an opaque stage ID.
            if field == "stepId" {
                value["payload"]["command"][field] = json!("https://wrong");
            }
            assert!(
                serde_json::from_value::<Envelope>(value)
                    .unwrap()
                    .validate(&identity, 2)
                    .is_err()
            );
        }
        assert!(envelope.validate(&identity, 3).is_err());
        let mut other = identity;
        other.user_id = "other-owner".into();
        assert!(envelope.validate(&other, 2).is_err());
    }
    #[test]
    fn expired_delivery_and_nonmatching_cancel_never_execute() {
        let (identity, mut envelope) = fixture();
        envelope.deadline = "2000-01-01T00:00:00Z".into();
        assert!(envelope.validate(&identity, 2).is_err());
        assert!(!cancellation_matches(
            &envelope,
            &json!({"type":"command.cancel","commandId":"cmd-1","taskId":"other-run"})
        ));
        assert!(cancellation_matches(
            &envelope,
            &json!({"type":"command.cancel","commandId":"cmd-1","taskId":"run-1"})
        ));
    }
    #[test]
    fn result_and_ack_keep_the_existing_backend_wire_shape() {
        let (identity, envelope) = fixture();
        let payload = envelope.terminal("completed", json!({"output":{"resourceId":"asset-1"}}));
        let frame = envelope.frame(&identity, 2, payload);
        assert_eq!(frame["result"]["kind"], "artcraft.workflow.step.result");
        assert_eq!(frame["result"]["status"], "success");
        assert_eq!(frame["attemptId"], 1);
        assert_eq!(frame["result"]["payload"]["runId"], "run-1");
        let ack = envelope.ack(&identity, 2, true, "");
        assert_eq!(ack["type"], "delivery.ack");
        assert_eq!(ack["ack"]["connectionGeneration"], 2);
    }
}
