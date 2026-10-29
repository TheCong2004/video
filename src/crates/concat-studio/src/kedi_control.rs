// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local stdio MCP administrative bridge. The process owner chooses the role;
//! tool arguments cannot grant privileges. This module opens no network listener.
use crate::kedi_live::{ControlAction, ControlRequest, KediLiveRuntime, Role};
use serde_json::{Value, json};

/// Maximum JSON-RPC request size, including all tool arguments.
pub const MAX_CONTROL_BYTES: usize = 16 * 1024;

/// Handle a bounded MCP/JSON-RPC request against the serialized runtime owner.
pub fn handle_rpc(
    runtime: &mut KediLiveRuntime,
    request: Value,
    role: Role,
    now: &str,
    now_ms: u64,
) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let error = |code: i32, message: &str| {
        Some(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
    };
    if serde_json::to_vec(&request).map_or(true, |v| v.len() > MAX_CONTROL_BYTES)
        || request.get("jsonrpc") != Some(&json!("2.0"))
    {
        return error(-32600, "Invalid request");
    }
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    if !request.as_object().is_some_and(|o| o.contains_key("id")) {
        return None;
    }
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"kedi-live-local","version":"0.2.7"}})
        }
        "ping" => json!({}),
        "tools/list" => {
            let tools=[("live.get_status","Read redacted local session status",false),("live.get_metrics","Read bounded live metrics",false),("live.list_pending_approvals","Read pending approval status",false),("live.start","Start the local session",true),("live.stop","Emergency stop; independent of provider completion",true),("live.operator_takeover","Cancel automatic speech and take control",true),("live.pause","Pause automatic speech",true),("live.resume","Resume automatic speech",true),("live.feature_product","Select an authoritative product revision",true),("live.enqueue_speech","Enqueue operator speech without financial claims",true),("live.simulate_event","DEV ONLY: enqueue a simulated comment",true)].into_iter().map(|(name,description,mutating)|json!({"name":name,"description":description,"inputSchema":if mutating{json!({"type":"object","required":["command_id","expected_revision"],"properties":{"command_id":{"type":"string"},"expected_revision":{"type":"integer","minimum":0},"sku":{"type":"string"},"revision":{"type":"integer","minimum":0},"text":{"type":"string","maxLength":1000}},"additionalProperties":false})}else{json!({"type":"object","properties":{},"additionalProperties":false})}})).collect::<Vec<_>>();
            let mut tools = tools;
            tools.push(json!({"name":"live.introduce_product","description":"Queue an introduction from current authoritative product facts","inputSchema":{"type":"object","required":["command_id","expected_revision"],"properties":{"command_id":{"type":"string"},"expected_revision":{"type":"integer","minimum":0}},"additionalProperties":false}}));
            json!({"tools":tools})
        }
        "tools/call" => {
            let params = &request["params"];
            let name = params["name"].as_str().unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let output = match name {
                "live.get_status" | "live.get_metrics" => Ok(json!(runtime.status())),
                "live.list_pending_approvals" => {
                    Ok(json!({"pending":[],"platform_writes_enabled":false}))
                }
                _ => {
                    if args.as_object().is_none_or(|o| {
                        o.keys().any(|key| {
                            !matches!(
                                key.as_str(),
                                "command_id" | "expected_revision" | "sku" | "revision" | "text"
                            )
                        })
                    }) {
                        return error(-32602, "Invalid tool arguments");
                    }
                    let action = match name {
                        "live.start" => ControlAction::Start,
                        "live.stop" => ControlAction::Stop,
                        "live.operator_takeover" => ControlAction::Takeover,
                        "live.pause" => ControlAction::Pause,
                        "live.resume" => ControlAction::Resume,
                        "live.introduce_product" => ControlAction::IntroduceProduct,
                        "live.feature_product" => {
                            let (Some(sku), Some(revision)) =
                                (args["sku"].as_str(), args["revision"].as_u64())
                            else {
                                return error(-32602, "sku and revision are required");
                            };
                            ControlAction::Feature {
                                sku: sku.into(),
                                revision,
                            }
                        }
                        "live.enqueue_speech" => {
                            let Some(text) = args["text"].as_str() else {
                                return error(-32602, "text is required");
                            };
                            ControlAction::Speak { text: text.into() }
                        }
                        "live.simulate_event" => {
                            let Some(text) = args["text"].as_str() else {
                                return error(-32602, "text is required");
                            };
                            ControlAction::Simulate { text: text.into() }
                        }
                        _ => return error(-32601, "Unknown tool"),
                    };
                    let Some(command_id) = args["command_id"].as_str() else {
                        return error(-32602, "command_id is required");
                    };
                    let Some(expected_revision) = args["expected_revision"].as_u64() else {
                        return error(-32602, "expected_revision is required");
                    };
                    runtime
                        .execute(
                            ControlRequest {
                                command_id: command_id.into(),
                                expected_revision,
                                action,
                            },
                            role,
                            now,
                            now_ms,
                        )
                        .map(|s| json!(s))
                }
            };
            match output {
                Ok(value) => {
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
                }
                Err(e) => {
                    json!({"content":[{"type":"text","text":format!("{e:?}")}],"isError":true})
                }
            }
        }
        _ => return error(-32601, "Unknown method"),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
