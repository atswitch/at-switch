use std::collections::BTreeMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::{ApiProtocol, AppResult, CommandError};

use super::CanonicalUsage;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanonicalStreamEvent {
    MessageStart {
        id: Option<String>,
        model: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ToolCallStart {
        index: u64,
        id: String,
        name: String,
    },
    ToolArgumentsDelta {
        index: u64,
        delta: String,
    },
    Usage {
        usage: CanonicalUsage,
    },
    MessageEnd {
        finish_reason: Option<String>,
    },
}

/// Decode one complete SSE event. The framing layer is responsible for
/// combining network chunks until the blank line that terminates an SSE event.
pub fn decode_stream_event(
    protocol: ApiProtocol,
    event_name: Option<&str>,
    data: &str,
) -> AppResult<Vec<CanonicalStreamEvent>> {
    if data.trim() == "[DONE]" {
        return Ok(vec![CanonicalStreamEvent::MessageEnd {
            finish_reason: Some("stop".to_owned()),
        }]);
    }
    let value: Value = serde_json::from_str(data).map_err(|_| {
        CommandError::new("invalid_stream_event", "上游返回了无法解析的流式 JSON 事件")
    })?;
    match protocol {
        ApiProtocol::OpenaiChatCompletions => decode_chat_event(&value),
        ApiProtocol::OpenaiResponses => decode_responses_event(
            event_name.or_else(|| value.get("type").and_then(Value::as_str)),
            &value,
        ),
        ApiProtocol::AnthropicMessages => decode_anthropic_event(
            event_name.or_else(|| value.get("type").and_then(Value::as_str)),
            &value,
        ),
    }
}

#[cfg(test)]
pub fn encode_stream_event(
    protocol: ApiProtocol,
    event: &CanonicalStreamEvent,
) -> AppResult<String> {
    let mut encoder = StreamEncoder::new(protocol);
    Ok(encoder.encode(event)?.concat())
}

/// Stateful SSE encoder used for one upstream response stream.
///
/// OpenAI Responses text and tool deltas are only meaningful after a matching
/// output item has been announced, and the completed response must contain the
/// accumulated output. Keeping this state per HTTP response also prevents
/// repeated Chat Completions metadata chunks (and the terminal `[DONE]`) from
/// producing duplicate Responses lifecycle events.
pub struct StreamEncoder {
    protocol: ApiProtocol,
    responses: ResponsesStreamState,
}

impl StreamEncoder {
    pub fn new(protocol: ApiProtocol) -> Self {
        Self {
            protocol,
            responses: ResponsesStreamState::default(),
        }
    }

    pub fn encode(&mut self, event: &CanonicalStreamEvent) -> AppResult<Vec<String>> {
        match self.protocol {
            ApiProtocol::OpenaiChatCompletions => Ok(vec![encode_chat_event(event)?]),
            ApiProtocol::OpenaiResponses => self.responses.encode(event),
            ApiProtocol::AnthropicMessages => Ok(vec![encode_anthropic_event(event)?]),
        }
    }
}

fn decode_chat_event(value: &Value) -> AppResult<Vec<CanonicalStreamEvent>> {
    let mut events = Vec::new();
    if value.get("id").is_some() || value.get("model").is_some() {
        events.push(CanonicalStreamEvent::MessageStart {
            id: string_at(value, "/id"),
            model: string_at(value, "/model"),
        });
    }
    if let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    {
        if let Some(text) = string_at(choice, "/delta/content") {
            events.push(CanonicalStreamEvent::TextDelta { text });
        }
        if let Some(tool_calls) = choice
            .pointer("/delta/tool_calls")
            .and_then(Value::as_array)
        {
            for tool in tool_calls {
                let index = tool.get("index").and_then(Value::as_u64).unwrap_or(0);
                if let (Some(id), Some(name)) = (
                    tool.get("id").and_then(Value::as_str),
                    tool.pointer("/function/name").and_then(Value::as_str),
                ) {
                    events.push(CanonicalStreamEvent::ToolCallStart {
                        index,
                        id: id.to_owned(),
                        name: name.to_owned(),
                    });
                }
                if let Some(delta) = tool.pointer("/function/arguments").and_then(Value::as_str) {
                    events.push(CanonicalStreamEvent::ToolArgumentsDelta {
                        index,
                        delta: delta.to_owned(),
                    });
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            events.push(CanonicalStreamEvent::MessageEnd {
                finish_reason: Some(reason.to_owned()),
            });
        }
    }
    if let Some(usage) = value.get("usage") {
        events.push(CanonicalStreamEvent::Usage {
            usage: CanonicalUsage {
                input_tokens: usage.get("prompt_tokens").and_then(Value::as_u64),
                output_tokens: usage.get("completion_tokens").and_then(Value::as_u64),
                cache_read_tokens: usage
                    .pointer("/prompt_tokens_details/cached_tokens")
                    .and_then(Value::as_u64),
            },
        });
    }
    Ok(events)
}

fn decode_responses_event(
    event_name: Option<&str>,
    value: &Value,
) -> AppResult<Vec<CanonicalStreamEvent>> {
    let event = event_name.unwrap_or("");
    let decoded = match event {
        "response.created" | "response.in_progress" => vec![CanonicalStreamEvent::MessageStart {
            id: string_at(value, "/response/id").or_else(|| string_at(value, "/id")),
            model: string_at(value, "/response/model").or_else(|| string_at(value, "/model")),
        }],
        "response.output_text.delta" => vec![CanonicalStreamEvent::TextDelta {
            text: string_at(value, "/delta").unwrap_or_default(),
        }],
        "response.output_item.added"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("function_call") =>
        {
            vec![CanonicalStreamEvent::ToolCallStart {
                index: value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                id: string_at(value, "/item/call_id")
                    .or_else(|| string_at(value, "/item/id"))
                    .unwrap_or_else(|| "call_atsw".to_owned()),
                name: string_at(value, "/item/name").unwrap_or_default(),
            }]
        }
        "response.function_call_arguments.delta" => {
            vec![CanonicalStreamEvent::ToolArgumentsDelta {
                index: value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                delta: string_at(value, "/delta").unwrap_or_default(),
            }]
        }
        "response.completed" => {
            let mut events = Vec::new();
            if let Some(usage) = value.pointer("/response/usage") {
                events.push(CanonicalStreamEvent::Usage {
                    usage: CanonicalUsage {
                        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
                        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
                        cache_read_tokens: usage
                            .pointer("/input_tokens_details/cached_tokens")
                            .and_then(Value::as_u64),
                    },
                });
            }
            events.push(CanonicalStreamEvent::MessageEnd {
                finish_reason: string_at(value, "/response/status")
                    .or_else(|| Some("completed".to_owned())),
            });
            events
        }
        "" => {
            return Err(CommandError::new(
                "stream_event_type_missing",
                "OpenAI Responses 流事件缺少 type",
            ))
        }
        // Events such as content_part.added carry structure but no text delta;
        // ignoring them is safe because their semantic data arrives in a
        // dedicated delta/completed event.
        _ => Vec::new(),
    };
    Ok(decoded)
}

fn decode_anthropic_event(
    event_name: Option<&str>,
    value: &Value,
) -> AppResult<Vec<CanonicalStreamEvent>> {
    let event = event_name.unwrap_or("");
    let decoded = match event {
        "message_start" => vec![CanonicalStreamEvent::MessageStart {
            id: string_at(value, "/message/id"),
            model: string_at(value, "/message/model"),
        }],
        "content_block_start"
            if value.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use") =>
        {
            vec![CanonicalStreamEvent::ToolCallStart {
                index: value.get("index").and_then(Value::as_u64).unwrap_or(0),
                id: string_at(value, "/content_block/id")
                    .unwrap_or_else(|| "toolu_atsw".to_owned()),
                name: string_at(value, "/content_block/name").unwrap_or_default(),
            }]
        }
        "content_block_delta"
            if value.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
        {
            vec![CanonicalStreamEvent::TextDelta {
                text: string_at(value, "/delta/text").unwrap_or_default(),
            }]
        }
        "content_block_delta"
            if value.pointer("/delta/type").and_then(Value::as_str) == Some("input_json_delta") =>
        {
            vec![CanonicalStreamEvent::ToolArgumentsDelta {
                index: value.get("index").and_then(Value::as_u64).unwrap_or(0),
                delta: string_at(value, "/delta/partial_json").unwrap_or_default(),
            }]
        }
        "message_delta" => {
            let mut events = Vec::new();
            if let Some(usage) = value.get("usage") {
                events.push(CanonicalStreamEvent::Usage {
                    usage: CanonicalUsage {
                        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
                        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
                        cache_read_tokens: usage
                            .get("cache_read_input_tokens")
                            .and_then(Value::as_u64),
                    },
                });
            }
            if let Some(reason) = string_at(value, "/delta/stop_reason") {
                events.push(CanonicalStreamEvent::MessageEnd {
                    finish_reason: Some(reason),
                });
            }
            events
        }
        "message_stop" => vec![CanonicalStreamEvent::MessageEnd {
            finish_reason: Some("end_turn".to_owned()),
        }],
        "" => {
            return Err(CommandError::new(
                "stream_event_type_missing",
                "Anthropic 流事件缺少 event/type",
            ))
        }
        _ => Vec::new(),
    };
    Ok(decoded)
}

fn encode_chat_event(event: &CanonicalStreamEvent) -> AppResult<String> {
    let data = match event {
        CanonicalStreamEvent::MessageStart { id, model } => json!({
            "id": id.clone().unwrap_or_else(|| "chatcmpl-atsw".to_owned()),
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{"index": 0, "delta": {"role": "assistant"}, "finish_reason": null}]
        }),
        CanonicalStreamEvent::TextDelta { text } => json!({
            "id": "chatcmpl-atsw",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
        }),
        CanonicalStreamEvent::ToolCallStart { index, id, name } => json!({
            "id": "chatcmpl-atsw",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"tool_calls": [{
                "index": index, "id": id, "type": "function",
                "function": {"name": name, "arguments": ""}
            }]}, "finish_reason": null}]
        }),
        CanonicalStreamEvent::ToolArgumentsDelta { index, delta } => json!({
            "id": "chatcmpl-atsw",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"tool_calls": [{
                "index": index, "function": {"arguments": delta}
            }]}, "finish_reason": null}]
        }),
        CanonicalStreamEvent::Usage { usage } => json!({
            "id": "chatcmpl-atsw",
            "object": "chat.completion.chunk",
            "choices": [],
            "usage": {
                "prompt_tokens": usage.input_tokens,
                "completion_tokens": usage.output_tokens
            }
        }),
        CanonicalStreamEvent::MessageEnd { finish_reason } => json!({
            "id": "chatcmpl-atsw",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": finish_reason}]
        }),
    };
    Ok(format!("data: {data}\n\n"))
}

#[derive(Default)]
struct ResponsesStreamState {
    response_id: Option<String>,
    model: Option<String>,
    created_at: Option<i64>,
    sequence_number: u64,
    created: bool,
    completed: bool,
    next_output_index: u64,
    text_output_index: Option<u64>,
    text: String,
    tools: BTreeMap<u64, ResponsesToolState>,
    usage: CanonicalUsage,
}

struct ResponsesToolState {
    output_index: u64,
    id: String,
    name: String,
    arguments: String,
}

impl ResponsesStreamState {
    fn encode(&mut self, event: &CanonicalStreamEvent) -> AppResult<Vec<String>> {
        if self.completed {
            return Ok(Vec::new());
        }
        match event {
            CanonicalStreamEvent::MessageStart { id, model } => {
                if self.response_id.is_none() {
                    self.response_id = id.as_deref().map(responses_id);
                }
                if self.model.is_none() {
                    self.model.clone_from(model);
                }
                self.ensure_created()
            }
            CanonicalStreamEvent::TextDelta { text } => {
                let mut frames = self.ensure_created()?;
                let output_index = match self.text_output_index {
                    Some(index) => index,
                    None => {
                        let index = self.take_output_index();
                        self.text_output_index = Some(index);
                        frames.push(self.frame(
                            "response.output_item.added",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": index,
                                "item": self.text_item("in_progress", "")
                            }),
                        ));
                        frames.push(self.frame(
                            "response.content_part.added",
                            json!({
                                "type": "response.content_part.added",
                                "item_id": self.message_id(),
                                "output_index": index,
                                "content_index": 0,
                                "part": output_text("")
                            }),
                        ));
                        index
                    }
                };
                self.text.push_str(text);
                frames.push(self.frame(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": self.message_id(),
                        "output_index": output_index,
                        "content_index": 0,
                        "delta": text,
                        "logprobs": []
                    }),
                ));
                Ok(frames)
            }
            CanonicalStreamEvent::ToolCallStart { index, id, name } => {
                let mut frames = self.ensure_created()?;
                if self.tools.contains_key(index) {
                    return Ok(frames);
                }
                let output_index = self.take_output_index();
                let state = ResponsesToolState {
                    output_index,
                    id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                };
                frames.push(self.frame(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "output_index": output_index,
                        "item": function_call_item(&state, "in_progress")
                    }),
                ));
                self.tools.insert(*index, state);
                Ok(frames)
            }
            CanonicalStreamEvent::ToolArgumentsDelta { index, delta } => {
                let frames = self.ensure_created()?;
                let Some(tool) = self.tools.get_mut(index) else {
                    return Err(CommandError::new(
                        "stream_tool_call_missing",
                        format!("Tool 参数增量缺少起始事件：index={index}"),
                    ));
                };
                tool.arguments.push_str(delta);
                let item_id = tool.id.clone();
                let output_index = tool.output_index;
                let mut frames = frames;
                frames.push(self.frame(
                    "response.function_call_arguments.delta",
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": item_id,
                        "output_index": output_index,
                        "delta": delta
                    }),
                ));
                Ok(frames)
            }
            CanonicalStreamEvent::Usage { usage } => {
                self.usage = usage.clone();
                Ok(Vec::new())
            }
            CanonicalStreamEvent::MessageEnd { .. } => self.complete(),
        }
    }

    fn ensure_created(&mut self) -> AppResult<Vec<String>> {
        if self.created {
            return Ok(Vec::new());
        }
        self.created = true;
        self.created_at = Some(Utc::now().timestamp());
        if self.response_id.is_none() {
            self.response_id = Some("resp_atsw".to_owned());
        }
        Ok(vec![self.frame(
            "response.created",
            json!({
                "type": "response.created",
                "response": self.response_object("in_progress", Vec::new(), false)
            }),
        )])
    }

    fn complete(&mut self) -> AppResult<Vec<String>> {
        if self.completed {
            return Ok(Vec::new());
        }
        let mut frames = self.ensure_created()?;
        let mut output = Vec::new();

        if let Some(output_index) = self.text_output_index {
            frames.push(self.frame(
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": self.message_id(),
                    "output_index": output_index,
                    "content_index": 0,
                    "text": self.text,
                    "logprobs": []
                }),
            ));
            frames.push(self.frame(
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "item_id": self.message_id(),
                    "output_index": output_index,
                    "content_index": 0,
                    "part": output_text(&self.text)
                }),
            ));
            let item = self.text_item("completed", &self.text);
            frames.push(self.frame(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item
                }),
            ));
            output.push((output_index, item));
        }

        let tool_indices = self.tools.keys().copied().collect::<Vec<_>>();
        for index in tool_indices {
            let tool = self
                .tools
                .get(&index)
                .ok_or_else(|| CommandError::internal("Tool 流状态丢失"))?;
            let output_index = tool.output_index;
            let item_id = tool.id.clone();
            let name = tool.name.clone();
            let arguments = tool.arguments.clone();
            let item = function_call_item(tool, "completed");
            frames.push(self.frame(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": item_id,
                    "name": name,
                    "output_index": output_index,
                    "arguments": arguments
                }),
            ));
            frames.push(self.frame(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item
                }),
            ));
            output.push((output_index, item));
        }
        output.sort_by_key(|(index, _)| *index);
        let output = output.into_iter().map(|(_, item)| item).collect();

        frames.push(self.frame(
            "response.completed",
            json!({
                "type": "response.completed",
                "response": self.response_object("completed", output, true)
            }),
        ));
        self.completed = true;
        Ok(frames)
    }

    fn frame(&mut self, name: &str, mut data: Value) -> String {
        data["sequence_number"] = json!(self.sequence_number);
        self.sequence_number += 1;
        format!("event: {name}\ndata: {data}\n\n")
    }

    fn take_output_index(&mut self) -> u64 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

    fn response_id(&self) -> &str {
        self.response_id.as_deref().unwrap_or("resp_atsw")
    }

    fn message_id(&self) -> String {
        format!("msg_{}", self.response_id().trim_start_matches("resp_"))
    }

    fn text_item(&self, status: &str, text: &str) -> Value {
        let content = if text.is_empty() {
            Vec::new()
        } else {
            vec![output_text(text)]
        };
        json!({
            "id": self.message_id(),
            "type": "message",
            "status": status,
            "role": "assistant",
            "content": content
        })
    }

    fn response_object(&self, status: &str, output: Vec<Value>, completed: bool) -> Value {
        let input_tokens = self.usage.input_tokens.unwrap_or(0);
        let output_tokens = self.usage.output_tokens.unwrap_or(0);
        json!({
            "id": self.response_id(),
            "object": "response",
            "created_at": self.created_at.unwrap_or(0),
            "completed_at": completed.then(|| Utc::now().timestamp()),
            "status": status,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "max_output_tokens": null,
            "model": self.model,
            "output": output,
            "parallel_tool_calls": true,
            "previous_response_id": null,
            "reasoning": null,
            "store": false,
            "temperature": 1.0,
            "text": {"format": {"type": "text"}},
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0,
            "truncation": "disabled",
            "usage": {
                "input_tokens": input_tokens,
                "input_tokens_details": {
                    "cached_tokens": self.usage.cache_read_tokens.unwrap_or(0)
                },
                "output_tokens": output_tokens,
                "output_tokens_details": {"reasoning_tokens": 0},
                "total_tokens": input_tokens + output_tokens
            },
            "user": null,
            "metadata": {}
        })
    }
}

fn responses_id(id: &str) -> String {
    if id.starts_with("resp_") {
        id.to_owned()
    } else {
        format!("resp_atsw_{id}")
    }
}

fn output_text(text: &str) -> Value {
    json!({
        "type": "output_text",
        "text": text,
        "annotations": [],
        "logprobs": []
    })
}

fn function_call_item(tool: &ResponsesToolState, status: &str) -> Value {
    json!({
        "id": tool.id,
        "type": "function_call",
        "status": status,
        "call_id": tool.id,
        "name": tool.name,
        "arguments": tool.arguments
    })
}

fn encode_anthropic_event(event: &CanonicalStreamEvent) -> AppResult<String> {
    let (name, data) = match event {
        CanonicalStreamEvent::MessageStart { id, model } => (
            "message_start",
            json!({"type": "message_start", "message": {
                "id": id.clone().unwrap_or_else(|| "msg_atsw".to_owned()),
                "type": "message", "role": "assistant", "model": model,
                "content": [], "stop_reason": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }}),
        ),
        CanonicalStreamEvent::TextDelta { text } => (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": text}}),
        ),
        CanonicalStreamEvent::ToolCallStart { index, id, name } => (
            "content_block_start",
            json!({"type": "content_block_start", "index": index,
                "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}}),
        ),
        CanonicalStreamEvent::ToolArgumentsDelta { index, delta } => (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": index,
                "delta": {"type": "input_json_delta", "partial_json": delta}}),
        ),
        CanonicalStreamEvent::Usage { usage } => (
            "message_delta",
            json!({"type": "message_delta", "delta": {"stop_reason": null},
                "usage": {"input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
                    "cache_read_input_tokens": usage.cache_read_tokens}}),
        ),
        CanonicalStreamEvent::MessageEnd { finish_reason } => (
            "message_delta",
            json!({"type": "message_delta",
                "delta": {"stop_reason": finish_reason.clone().unwrap_or_else(|| "end_turn".to_owned())},
                "usage": {"output_tokens": 0}}),
        ),
    };
    Ok(format!("event: {name}\ndata: {data}\n\n"))
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta_converts_across_all_nine_stream_directions() {
        let protocols = [
            ApiProtocol::OpenaiChatCompletions,
            ApiProtocol::OpenaiResponses,
            ApiProtocol::AnthropicMessages,
        ];
        let fixtures = [
            (
                None,
                r#"{"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}"#,
            ),
            (
                Some("response.output_text.delta"),
                r#"{"type":"response.output_text.delta","delta":"hello"}"#,
            ),
            (
                Some("content_block_delta"),
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#,
            ),
        ];

        for (source_index, source) in protocols.iter().enumerate() {
            let (name, data) = fixtures[source_index];
            let events = decode_stream_event(*source, name, data).expect("decode");
            assert_eq!(
                events,
                vec![CanonicalStreamEvent::TextDelta {
                    text: "hello".to_owned()
                }]
            );
            for target in protocols {
                let encoded = encode_stream_event(target, &events[0]).expect("encode");
                assert!(encoded.contains("hello"));
                assert!(encoded.ends_with("\n\n"));
            }
        }
    }

    #[test]
    fn malformed_stream_json_is_an_explicit_conversion_error() {
        let error = decode_stream_event(ApiProtocol::OpenaiChatCompletions, None, "{not-json}")
            .expect_err("must reject");
        assert_eq!(error.code, "invalid_stream_event");
    }

    #[test]
    fn chat_stream_builds_a_complete_responses_assistant_item() {
        let chunks = [
            r#"{"id":"chatcmpl-test","model":"glm-test","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            r#"{"id":"chatcmpl-test","model":"glm-test","choices":[{"index":0,"delta":{"content":"AT_SWITCH_"},"finish_reason":null}]}"#,
            r#"{"id":"chatcmpl-test","model":"glm-test","choices":[{"index":0,"delta":{"content":"E2E_OK"},"finish_reason":null}]}"#,
            r#"{"id":"chatcmpl-test","model":"glm-test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#,
            "[DONE]",
        ];
        let mut encoder = StreamEncoder::new(ApiProtocol::OpenaiResponses);
        let mut frames = Vec::new();

        for chunk in chunks {
            for event in decode_stream_event(ApiProtocol::OpenaiChatCompletions, None, chunk)
                .expect("decode")
            {
                frames.extend(encoder.encode(&event).expect("encode"));
            }
        }

        let events = frames
            .iter()
            .map(|frame| {
                let event = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .expect("event name");
                let data = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .expect("event data");
                (
                    event.to_owned(),
                    serde_json::from_str::<Value>(data).expect("event json"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            events
                .iter()
                .map(|(event, _)| event.as_str())
                .collect::<Vec<_>>(),
            vec![
                "response.created",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        for (sequence, (_, data)) in events.iter().enumerate() {
            assert_eq!(
                data.get("sequence_number").and_then(Value::as_u64),
                Some(sequence as u64)
            );
        }
        let completed = &events.last().expect("completed").1;
        assert_eq!(
            completed
                .pointer("/response/status")
                .and_then(Value::as_str),
            Some("completed")
        );
        assert_eq!(
            completed
                .pointer("/response/output/0/content/0/text")
                .and_then(Value::as_str),
            Some("AT_SWITCH_E2E_OK")
        );
        assert_eq!(
            completed
                .pointer("/response/output/0/role")
                .and_then(Value::as_str),
            Some("assistant")
        );
    }
}
