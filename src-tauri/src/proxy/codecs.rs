use serde_json::{json, Value};

use crate::domain::{ApiProtocol, AppResult, CommandError};

use super::{
    CanonicalBlock, CanonicalMessage, CanonicalRequest, CanonicalResponse, CanonicalRole,
    CanonicalTool, CanonicalUsage,
};

pub fn decode_request(protocol: ApiProtocol, value: &Value) -> AppResult<CanonicalRequest> {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => decode_chat_request(value),
        ApiProtocol::OpenaiResponses => decode_responses_request(value),
        ApiProtocol::AnthropicMessages => decode_anthropic_request(value),
    }
}

pub fn encode_request(protocol: ApiProtocol, request: &CanonicalRequest) -> AppResult<Value> {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => encode_chat_request(request),
        ApiProtocol::OpenaiResponses => encode_responses_request(request),
        ApiProtocol::AnthropicMessages => encode_anthropic_request(request),
    }
}

pub fn decode_response(protocol: ApiProtocol, value: &Value) -> AppResult<CanonicalResponse> {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => decode_chat_response(value),
        ApiProtocol::OpenaiResponses => decode_responses_response(value),
        ApiProtocol::AnthropicMessages => decode_anthropic_response(value),
    }
}

pub fn encode_response(protocol: ApiProtocol, response: &CanonicalResponse) -> AppResult<Value> {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => encode_chat_response(response),
        ApiProtocol::OpenaiResponses => encode_responses_response(response),
        ApiProtocol::AnthropicMessages => encode_anthropic_response(response),
    }
}

fn decode_chat_request(value: &Value) -> AppResult<CanonicalRequest> {
    let model = required_string(value, "model")?;
    let mut system = None;
    let mut messages = Vec::new();
    for message in required_array(value, "messages")? {
        let role = required_string(message, "role")?;
        if role == "system" || role == "developer" {
            let text = content_text(message.get("content"))?;
            append_system(&mut system, &text);
            continue;
        }
        if role == "tool" {
            messages.push(CanonicalMessage {
                role: CanonicalRole::User,
                content: vec![CanonicalBlock::ToolResult {
                    call_id: required_string(message, "tool_call_id")?,
                    content: content_text(message.get("content"))?,
                    is_error: false,
                }],
            });
            continue;
        }
        let canonical_role = match role.as_str() {
            "user" => CanonicalRole::User,
            "assistant" => CanonicalRole::Assistant,
            _ => {
                return Err(unsupported(format!(
                    "OpenAI Chat 消息角色 `{role}` 尚不支持"
                )))
            }
        };
        let mut blocks = decode_openai_content(message.get("content"))?;
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let function = call
                    .get("function")
                    .ok_or_else(|| invalid_request("OpenAI Chat tool_call 缺少 function"))?;
                let arguments_text = required_string(function, "arguments")?;
                let arguments = parse_json_arguments(&arguments_text)?;
                blocks.push(CanonicalBlock::ToolCall {
                    id: required_string(call, "id")?,
                    name: required_string(function, "name")?,
                    arguments,
                });
            }
        }
        messages.push(CanonicalMessage {
            role: canonical_role,
            content: blocks,
        });
    }

    Ok(CanonicalRequest {
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        model,
        system,
        messages,
        stream: value
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        max_tokens: value
            .get("max_tokens")
            .or_else(|| value.get("max_completion_tokens"))
            .and_then(Value::as_u64),
        temperature: value.get("temperature").and_then(Value::as_f64),
        top_p: value.get("top_p").and_then(Value::as_f64),
        stop: decode_stop(value.get("stop"))?,
        tools: decode_chat_tools(value.get("tools"))?,
        responses_native_tools: Vec::new(),
        tool_choice: value.get("tool_choice").cloned(),
    })
}

fn decode_responses_request(value: &Value) -> AppResult<CanonicalRequest> {
    let model = required_string(value, "model")?;
    let mut system = value
        .get("instructions")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mut messages = Vec::new();
    match value.get("input") {
        Some(Value::String(text)) => messages.push(CanonicalMessage {
            role: CanonicalRole::User,
            content: vec![CanonicalBlock::Text { text: text.clone() }],
        }),
        Some(Value::Array(items)) => {
            for item in items {
                let item_type = item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("message");
                match item_type {
                    "message" => {
                        let source_role = required_string(item, "role")?;
                        if source_role == "system" || source_role == "developer" {
                            let text = content_text(item.get("content"))?;
                            append_system(&mut system, &text);
                            continue;
                        }
                        let role = match source_role.as_str() {
                            "user" => CanonicalRole::User,
                            "assistant" => CanonicalRole::Assistant,
                            role => {
                                return Err(unsupported(format!(
                                    "OpenAI Responses 消息角色 `{role}` 尚不支持"
                                )))
                            }
                        };
                        messages.push(CanonicalMessage {
                            role,
                            content: decode_openai_content(item.get("content"))?,
                        });
                    }
                    "function_call" => messages.push(CanonicalMessage {
                        role: CanonicalRole::Assistant,
                        content: vec![CanonicalBlock::ToolCall {
                            id: item
                                .get("call_id")
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    invalid_request("Responses function_call 缺少 call_id")
                                })?
                                .to_owned(),
                            name: required_string(item, "name")?,
                            arguments: parse_json_arguments(&required_string(item, "arguments")?)?,
                        }],
                    }),
                    "function_call_output" => messages.push(CanonicalMessage {
                        role: CanonicalRole::User,
                        content: vec![CanonicalBlock::ToolResult {
                            call_id: required_string(item, "call_id")?,
                            content: value_to_text(item.get("output"))?,
                            is_error: false,
                        }],
                    }),
                    other => {
                        return Err(unsupported(format!(
                            "OpenAI Responses input item `{other}` 不能安全转换"
                        )))
                    }
                }
            }
        }
        _ => return Err(invalid_request("OpenAI Responses 请求缺少 input")),
    }

    let (tools, responses_native_tools) = decode_responses_tools(value.get("tools"))?;

    Ok(CanonicalRequest {
        source_protocol: ApiProtocol::OpenaiResponses,
        model,
        system,
        messages,
        stream: value
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        max_tokens: value
            .get("max_output_tokens")
            .or_else(|| value.get("max_tokens"))
            .and_then(Value::as_u64),
        temperature: value.get("temperature").and_then(Value::as_f64),
        top_p: value.get("top_p").and_then(Value::as_f64),
        stop: decode_stop(value.get("stop"))?,
        tools,
        responses_native_tools,
        tool_choice: value.get("tool_choice").cloned(),
    })
}

fn decode_anthropic_request(value: &Value) -> AppResult<CanonicalRequest> {
    let model = required_string(value, "model")?;
    let system = value.get("system").map(system_text).transpose()?;
    let mut messages = Vec::new();
    for message in required_array(value, "messages")? {
        let role = match required_string(message, "role")?.as_str() {
            "user" => CanonicalRole::User,
            "assistant" => CanonicalRole::Assistant,
            role => return Err(unsupported(format!("Anthropic 消息角色 `{role}` 尚不支持"))),
        };
        messages.push(CanonicalMessage {
            role,
            content: decode_anthropic_content(message.get("content"))?,
        });
    }
    let tools = value
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    Ok(CanonicalTool {
                        name: required_string(tool, "name")?,
                        description: tool
                            .get("description")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        input_schema: tool
                            .get("input_schema")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object"})),
                    })
                })
                .collect::<AppResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(CanonicalRequest {
        source_protocol: ApiProtocol::AnthropicMessages,
        model,
        system,
        messages,
        stream: value
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        max_tokens: value.get("max_tokens").and_then(Value::as_u64),
        temperature: value.get("temperature").and_then(Value::as_f64),
        top_p: value.get("top_p").and_then(Value::as_f64),
        stop: decode_stop(value.get("stop_sequences"))?,
        tools,
        responses_native_tools: Vec::new(),
        tool_choice: value.get("tool_choice").cloned(),
    })
}

fn encode_chat_request(request: &CanonicalRequest) -> AppResult<Value> {
    let mut messages = Vec::new();
    let system = system_with_native_tool_compatibility_notice(request, "OpenAI Chat Completions");
    if let Some(system) = system {
        messages.push(json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        let role = match message.role {
            CanonicalRole::User => "user",
            CanonicalRole::Assistant => "assistant",
        };
        let text = joined_text(&message.content);
        let tool_calls = message
            .content
            .iter()
            .filter_map(|block| match block {
                CanonicalBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } => Some(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_owned())
                    }
                })),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = message
            .content
            .iter()
            .filter_map(|block| match block {
                CanonicalBlock::ToolResult {
                    call_id, content, ..
                } => Some(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": content
                })),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !text.is_empty() || !tool_calls.is_empty() {
            let mut encoded = json!({"role": role, "content": text});
            if !tool_calls.is_empty() {
                encoded["tool_calls"] = Value::Array(tool_calls);
            }
            messages.push(encoded);
        }
        messages.extend(tool_results);
    }
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema
                }
            })
        })
        .collect::<Vec<_>>();
    let mut output = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream
    });
    put_common_parameters(&mut output, request, "max_tokens", "stop")?;
    if !tools.is_empty() {
        output["tools"] = Value::Array(tools);
    }
    if let Some(choice) =
        normalize_chat_tool_choice(request.tool_choice.as_ref(), !request.tools.is_empty())?
    {
        output["tool_choice"] = choice;
    }
    Ok(output)
}

fn encode_responses_request(request: &CanonicalRequest) -> AppResult<Value> {
    let mut input = Vec::new();
    for message in &request.messages {
        let role = match message.role {
            CanonicalRole::User => "user",
            CanonicalRole::Assistant => "assistant",
        };
        let text = joined_text(&message.content);
        if !text.is_empty() {
            input.push(json!({
                "type": "message",
                "role": role,
                "content": [{
                    "type": if role == "assistant" { "output_text" } else { "input_text" },
                    "text": text
                }]
            }));
        }
        for block in &message.content {
            match block {
                CanonicalBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } => input.push(json!({
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_owned())
                })),
                CanonicalBlock::ToolResult {
                    call_id, content, ..
                } => input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": content
                })),
                CanonicalBlock::Text { .. } => {}
            }
        }
    }
    let mut tools = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
                "strict": false
            })
        })
        .collect::<Vec<_>>();
    tools.extend(request.responses_native_tools.iter().cloned());
    let mut output = json!({
        "model": request.model,
        "input": input,
        "stream": request.stream
    });
    if let Some(system) = &request.system {
        output["instructions"] = Value::String(system.clone());
    }
    put_common_parameters(&mut output, request, "max_output_tokens", "stop")?;
    if !tools.is_empty() {
        output["tools"] = Value::Array(tools);
    }
    if let Some(choice) = &request.tool_choice {
        output["tool_choice"] = choice.clone();
    }
    Ok(output)
}

fn encode_anthropic_request(request: &CanonicalRequest) -> AppResult<Value> {
    let messages = request
        .messages
        .iter()
        .map(|message| {
            let role = match message.role {
                CanonicalRole::User => "user",
                CanonicalRole::Assistant => "assistant",
            };
            let content = message
                .content
                .iter()
                .map(|block| match block {
                    CanonicalBlock::Text { text } => json!({
                        "type": "text",
                        "text": text
                    }),
                    CanonicalBlock::ToolCall {
                        id,
                        name,
                        arguments,
                    } => json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": arguments
                    }),
                    CanonicalBlock::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } => json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": content,
                        "is_error": is_error
                    }),
                })
                .collect::<Vec<_>>();
            json!({"role": role, "content": content})
        })
        .collect::<Vec<_>>();
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema
            })
        })
        .collect::<Vec<_>>();
    let mut output = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(1024),
        "stream": request.stream
    });
    if let Some(system) =
        system_with_native_tool_compatibility_notice(request, "Anthropic Messages")
    {
        output["system"] = Value::String(system);
    }
    if let Some(temperature) = request.temperature {
        output["temperature"] = json!(temperature);
    }
    if let Some(top_p) = request.top_p {
        output["top_p"] = json!(top_p);
    }
    if !request.stop.is_empty() {
        output["stop_sequences"] = json!(request.stop);
    }
    if !tools.is_empty() {
        output["tools"] = Value::Array(tools);
    }
    if let Some(choice) =
        normalize_anthropic_tool_choice(request.tool_choice.as_ref(), !request.tools.is_empty())?
    {
        output["tool_choice"] = choice;
    }
    Ok(output)
}

fn decode_chat_response(value: &Value) -> AppResult<CanonicalResponse> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| invalid_response("OpenAI Chat 响应缺少 choices"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| invalid_response("OpenAI Chat 响应缺少 message"))?;
    let mut content = decode_openai_content(message.get("content"))?;
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let function = call
                .get("function")
                .ok_or_else(|| invalid_response("Tool Call 缺少 function"))?;
            content.push(CanonicalBlock::ToolCall {
                id: required_string(call, "id")?,
                name: required_string(function, "name")?,
                arguments: parse_json_arguments(&required_string(function, "arguments")?)?,
            });
        }
    }
    Ok(CanonicalResponse {
        id: optional_string(value, "id"),
        model: optional_string(value, "model"),
        content,
        finish_reason: optional_string(choice, "finish_reason"),
        usage: CanonicalUsage {
            input_tokens: value
                .pointer("/usage/prompt_tokens")
                .and_then(Value::as_u64),
            output_tokens: value
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_u64),
            cache_read_tokens: value
                .pointer("/usage/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_u64),
        },
    })
}

fn decode_responses_response(value: &Value) -> AppResult<CanonicalResponse> {
    let mut content = Vec::new();
    for item in value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("OpenAI Responses 响应缺少 output"))?
    {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "message" => content.extend(decode_openai_content(item.get("content"))?),
            "function_call" => content.push(CanonicalBlock::ToolCall {
                id: item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_response("function_call 缺少 call_id"))?
                    .to_owned(),
                name: required_string(item, "name")?,
                arguments: parse_json_arguments(&required_string(item, "arguments")?)?,
            }),
            other => {
                return Err(unsupported(format!(
                    "Responses 输出 `{other}` 不能安全转换"
                )))
            }
        }
    }
    Ok(CanonicalResponse {
        id: optional_string(value, "id"),
        model: optional_string(value, "model"),
        content,
        finish_reason: optional_string(value, "status"),
        usage: CanonicalUsage {
            input_tokens: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
            output_tokens: value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
            cache_read_tokens: value
                .pointer("/usage/input_tokens_details/cached_tokens")
                .and_then(Value::as_u64),
        },
    })
}

fn decode_anthropic_response(value: &Value) -> AppResult<CanonicalResponse> {
    Ok(CanonicalResponse {
        id: optional_string(value, "id"),
        model: optional_string(value, "model"),
        content: decode_anthropic_content(value.get("content"))?,
        finish_reason: optional_string(value, "stop_reason"),
        usage: CanonicalUsage {
            input_tokens: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
            output_tokens: value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
            cache_read_tokens: value
                .pointer("/usage/cache_read_input_tokens")
                .and_then(Value::as_u64),
        },
    })
}

fn encode_chat_response(response: &CanonicalResponse) -> AppResult<Value> {
    let text = joined_text(&response.content);
    let tool_calls = response
        .content
        .iter()
        .filter_map(|block| match block {
            CanonicalBlock::ToolCall {
                id,
                name,
                arguments,
            } => Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_owned())
                }
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut message = json!({"role": "assistant", "content": text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    Ok(json!({
        "id": response.id.clone().unwrap_or_else(|| "atsw-response".to_owned()),
        "object": "chat.completion",
        "model": response.model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": response.finish_reason
        }],
        "usage": {
            "prompt_tokens": response.usage.input_tokens,
            "completion_tokens": response.usage.output_tokens,
            "total_tokens": sum_usage(&response.usage)
        }
    }))
}

fn encode_responses_response(response: &CanonicalResponse) -> AppResult<Value> {
    let mut output = Vec::new();
    let text = joined_text(&response.content);
    if !text.is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}]
        }));
    }
    for block in &response.content {
        if let CanonicalBlock::ToolCall {
            id,
            name,
            arguments,
        } = block
        {
            output.push(json!({
                "type": "function_call",
                "call_id": id,
                "name": name,
                "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_owned())
            }));
        }
    }
    Ok(json!({
        "id": response.id.clone().unwrap_or_else(|| "resp_atsw".to_owned()),
        "object": "response",
        "status": response.finish_reason.clone().unwrap_or_else(|| "completed".to_owned()),
        "model": response.model,
        "output": output,
        "usage": {
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
            "total_tokens": sum_usage(&response.usage)
        }
    }))
}

fn encode_anthropic_response(response: &CanonicalResponse) -> AppResult<Value> {
    let content = response
        .content
        .iter()
        .filter_map(|block| match block {
            CanonicalBlock::Text { text } => Some(json!({"type": "text", "text": text})),
            CanonicalBlock::ToolCall {
                id,
                name,
                arguments,
            } => Some(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": arguments
            })),
            CanonicalBlock::ToolResult { .. } => None,
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "id": response.id.clone().unwrap_or_else(|| "msg_atsw".to_owned()),
        "type": "message",
        "role": "assistant",
        "model": response.model,
        "content": content,
        "stop_reason": response.finish_reason,
        "usage": {
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
            "cache_read_input_tokens": response.usage.cache_read_tokens
        }
    }))
}

fn decode_openai_content(value: Option<&Value>) -> AppResult<Vec<CanonicalBlock>> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![CanonicalBlock::Text { text: text.clone() }]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| {
                let part_type = part.get("type").and_then(Value::as_str).unwrap_or("text");
                match part_type {
                    "text" | "input_text" | "output_text" => Ok(CanonicalBlock::Text {
                        text: required_string(part, "text")?,
                    }),
                    other => Err(unsupported(format!(
                        "内容块 `{other}` 首版不能等价转换，已阻止请求"
                    ))),
                }
            })
            .collect(),
        _ => Err(invalid_request("消息 content 格式无效")),
    }
}

fn decode_anthropic_content(value: Option<&Value>) -> AppResult<Vec<CanonicalBlock>> {
    match value {
        Some(Value::String(text)) => Ok(vec![CanonicalBlock::Text { text: text.clone() }]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(
                |part| match part.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => Ok(CanonicalBlock::Text {
                        text: required_string(part, "text")?,
                    }),
                    "tool_use" => Ok(CanonicalBlock::ToolCall {
                        id: required_string(part, "id")?,
                        name: required_string(part, "name")?,
                        arguments: part.get("input").cloned().unwrap_or_else(|| json!({})),
                    }),
                    "tool_result" => Ok(CanonicalBlock::ToolResult {
                        call_id: required_string(part, "tool_use_id")?,
                        content: value_to_text(part.get("content"))?,
                        is_error: part
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    }),
                    other => Err(unsupported(format!(
                        "Anthropic 内容块 `{other}` 首版不能等价转换"
                    ))),
                },
            )
            .collect(),
        _ => Err(invalid_request("Anthropic 消息 content 格式无效")),
    }
}

fn decode_chat_tools(value: Option<&Value>) -> AppResult<Vec<CanonicalTool>> {
    value
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    if tool.get("type").and_then(Value::as_str) != Some("function") {
                        return Err(unsupported("只支持 function 类型的 Tool"));
                    }
                    let function = tool
                        .get("function")
                        .ok_or_else(|| invalid_request("Tool 缺少 function"))?;
                    Ok(CanonicalTool {
                        name: required_string(function, "name")?,
                        description: optional_string(function, "description"),
                        input_schema: function
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object"})),
                    })
                })
                .collect()
        })
        .transpose()
        .map(|tools| tools.unwrap_or_default())
}

fn decode_responses_tools(value: Option<&Value>) -> AppResult<(Vec<CanonicalTool>, Vec<Value>)> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let tools = value
        .as_array()
        .ok_or_else(|| invalid_request("Responses tools 必须是数组"))?;
    let mut function_tools = Vec::new();
    let mut native_tools = Vec::new();
    for tool in tools {
        match tool.get("type").and_then(Value::as_str) {
            Some("function") => function_tools.push(CanonicalTool {
                name: required_string(tool, "name")?,
                description: optional_string(tool, "description"),
                input_schema: tool
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
            }),
            Some(_) => native_tools.push(tool.clone()),
            None => return Err(invalid_request("Responses Tool 缺少字符串字段 `type`")),
        }
    }
    Ok((function_tools, native_tools))
}

fn put_common_parameters(
    output: &mut Value,
    request: &CanonicalRequest,
    max_tokens_name: &str,
    stop_name: &str,
) -> AppResult<()> {
    let object = output
        .as_object_mut()
        .ok_or_else(|| CommandError::internal("编码后的请求不是对象"))?;
    if let Some(max_tokens) = request.max_tokens {
        object.insert(max_tokens_name.to_owned(), json!(max_tokens));
    }
    if let Some(temperature) = request.temperature {
        object.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        object.insert("top_p".to_owned(), json!(top_p));
    }
    if !request.stop.is_empty() {
        object.insert(stop_name.to_owned(), json!(request.stop));
    }
    Ok(())
}

fn normalize_anthropic_tool_choice(
    value: Option<&Value>,
    has_function_tools: bool,
) -> AppResult<Option<Value>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::String(choice) if choice == "auto" => Ok(Some(json!({"type": "auto"}))),
        Value::String(choice) if choice == "required" && has_function_tools => {
            Ok(Some(json!({"type": "any"})))
        }
        Value::String(choice) if choice == "required" => Err(unsupported(
            "请求要求必须调用 Tool，但 Anthropic 上游没有可转换的 function Tool",
        )),
        Value::String(choice) if choice == "none" => Err(unsupported(
            "Anthropic Messages 没有与 tool_choice=none 完全等价的表示",
        )),
        Value::Object(_) => {
            if let Some(function_name) = value.pointer("/function/name").and_then(Value::as_str) {
                Ok(Some(json!({"type": "tool", "name": function_name})))
            } else if value.get("type").and_then(Value::as_str) == Some("function") {
                let function_name = required_string(value, "name")?;
                Ok(Some(json!({"type": "tool", "name": function_name})))
            } else if matches!(
                value.get("type").and_then(Value::as_str),
                Some("auto" | "any" | "tool")
            ) {
                Ok(Some(value.clone()))
            } else {
                Err(native_tool_choice_unsupported("Anthropic Messages", value))
            }
        }
        _ => Err(invalid_request("tool_choice 格式无效")),
    }
}

fn normalize_chat_tool_choice(
    value: Option<&Value>,
    has_function_tools: bool,
) -> AppResult<Option<Value>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::String(choice) if matches!(choice.as_str(), "auto" | "none") => {
            Ok(Some(value.clone()))
        }
        Value::String(choice) if choice == "required" && has_function_tools => {
            Ok(Some(value.clone()))
        }
        Value::String(choice) if choice == "required" => Err(unsupported(
            "请求要求必须调用 Tool，但 OpenAI Chat 上游没有可转换的 function Tool",
        )),
        Value::Object(_) if value.pointer("/function/name").is_some() => Ok(Some(value.clone())),
        Value::Object(_) if value.get("type").and_then(Value::as_str) == Some("function") => {
            let function_name = required_string(value, "name")?;
            Ok(Some(json!({
                "type": "function",
                "function": {"name": function_name}
            })))
        }
        Value::Object(_) => Err(native_tool_choice_unsupported(
            "OpenAI Chat Completions",
            value,
        )),
        _ => Err(invalid_request("tool_choice 格式无效")),
    }
}

fn native_tool_choice_unsupported(target: &str, value: &Value) -> CommandError {
    let tool_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    unsupported(format!(
        "请求强制使用 Responses 原生 Tool `{tool_type}`，但当前 Provider 使用 {target} 协议"
    ))
}

fn system_with_native_tool_compatibility_notice(
    request: &CanonicalRequest,
    target: &str,
) -> Option<String> {
    if request.responses_native_tools.is_empty() {
        return request.system.clone();
    }
    let mut native_types = request
        .responses_native_tools
        .iter()
        .filter_map(|tool| tool.get("type").and_then(Value::as_str))
        .collect::<Vec<_>>();
    native_types.sort_unstable();
    native_types.dedup();
    let notice = format!(
        "AT-Switch compatibility notice: the selected {target} provider cannot execute \
OpenAI Responses-native tools ({}). These optional tools are unavailable for this request. \
Use only the declared function tools.",
        native_types.join(", ")
    );
    match &request.system {
        Some(system) if !system.is_empty() => Some(format!("{system}\n{notice}")),
        _ => Some(notice),
    }
}

fn system_text(value: &Value) -> AppResult<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Array(parts) => Ok(parts
            .iter()
            .map(|part| required_string(part, "text"))
            .collect::<AppResult<Vec<_>>>()?
            .join("\n")),
        _ => Err(invalid_request("system 格式无效")),
    }
}

fn content_text(value: Option<&Value>) -> AppResult<String> {
    Ok(decode_openai_content(value)?
        .iter()
        .filter_map(|block| match block {
            CanonicalBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(""))
}

fn value_to_text(value: Option<&Value>) -> AppResult<String> {
    match value {
        None => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(value) => serde_json::to_string(value)
            .map_err(|_| invalid_request("无法把 Tool Result 转换为文本")),
    }
}

fn joined_text(blocks: &[CanonicalBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            CanonicalBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn parse_json_arguments(value: &str) -> AppResult<Value> {
    serde_json::from_str(value).map_err(|_| {
        CommandError::new(
            "tool_arguments_invalid",
            "Tool Call 参数不是合法 JSON，已阻止转换",
        )
    })
}

fn decode_stop(value: Option<&Value>) -> AppResult<Vec<String>> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(value)) => Ok(vec![value.clone()]),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| invalid_request("stop 必须是字符串数组"))
            })
            .collect(),
        _ => Err(invalid_request("stop 格式无效")),
    }
}

fn append_system(system: &mut Option<String>, text: &str) {
    match system {
        Some(existing) if !text.is_empty() => {
            existing.push('\n');
            existing.push_str(text);
        }
        None if !text.is_empty() => *system = Some(text.to_owned()),
        _ => {}
    }
}

fn required_string(value: &Value, field: &str) -> AppResult<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_request(format!("缺少字符串字段 `{field}`")))
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn required_array<'a>(value: &'a Value, field: &str) -> AppResult<&'a Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_request(format!("缺少数组字段 `{field}`")))
}

fn sum_usage(usage: &CanonicalUsage) -> Option<u64> {
    match (usage.input_tokens, usage.output_tokens) {
        (Some(input), Some(output)) => Some(input + output),
        _ => None,
    }
}

fn invalid_request(message: impl Into<String>) -> CommandError {
    CommandError::new("invalid_request", message)
}

fn invalid_response(message: impl Into<String>) -> CommandError {
    CommandError::new("invalid_upstream_response", message)
}

fn unsupported(message: impl Into<String>) -> CommandError {
    CommandError::new("unsupported_capability", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request(protocol: ApiProtocol) -> Value {
        match protocol {
            ApiProtocol::OpenaiChatCompletions => json!({
                "model": "source-model",
                "messages": [
                    {"role": "system", "content": "Be concise."},
                    {"role": "user", "content": "Hello"}
                ],
                "stream": false,
                "max_tokens": 32,
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "description": "Look something up",
                        "parameters": {"type": "object", "properties": {}}
                    }
                }]
            }),
            ApiProtocol::OpenaiResponses => json!({
                "model": "source-model",
                "instructions": "Be concise.",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "Hello"}]
                }],
                "max_output_tokens": 32,
                "tools": [{
                    "type": "function",
                    "name": "lookup",
                    "description": "Look something up",
                    "parameters": {"type": "object", "properties": {}}
                }]
            }),
            ApiProtocol::AnthropicMessages => json!({
                "model": "source-model",
                "system": "Be concise.",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "text", "text": "Hello"}]
                }],
                "max_tokens": 32,
                "tools": [{
                    "name": "lookup",
                    "description": "Look something up",
                    "input_schema": {"type": "object", "properties": {}}
                }]
            }),
        }
    }

    #[test]
    fn all_nine_request_directions_preserve_the_core_semantics() {
        let protocols = [
            ApiProtocol::OpenaiChatCompletions,
            ApiProtocol::OpenaiResponses,
            ApiProtocol::AnthropicMessages,
        ];
        for source in protocols {
            let canonical = decode_request(source, &sample_request(source)).expect("decode");
            assert_eq!(canonical.system.as_deref(), Some("Be concise."));
            assert_eq!(canonical.tools.len(), 1);
            for target in protocols {
                let encoded = encode_request(target, &canonical).expect("encode");
                let round_trip = decode_request(target, &encoded).expect("round trip");
                assert_eq!(round_trip.system, canonical.system);
                assert_eq!(round_trip.messages, canonical.messages);
                assert_eq!(round_trip.tools, canonical.tools);
                assert_eq!(round_trip.max_tokens, canonical.max_tokens);
            }
        }
    }

    #[test]
    fn unsupported_multimodal_content_is_never_silently_dropped() {
        let request = json!({
            "model": "model",
            "messages": [{
                "role": "user",
                "content": [{"type": "image_url", "image_url": {"url": "https://example.test/a.png"}}]
            }]
        });
        let error =
            decode_request(ApiProtocol::OpenaiChatCompletions, &request).expect_err("must reject");
        assert_eq!(error.code, "unsupported_capability");
    }

    #[test]
    fn responses_developer_messages_are_preserved_as_system_instructions() {
        let request = json!({
            "model": "source-model",
            "instructions": "Base instruction.",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{"type": "input_text", "text": "Use repository rules."}]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "Fix the test."}]
                }
            ]
        });

        let canonical =
            decode_request(ApiProtocol::OpenaiResponses, &request).expect("decode responses");
        assert_eq!(
            canonical.system.as_deref(),
            Some("Base instruction.\nUse repository rules.")
        );
        assert_eq!(canonical.messages.len(), 1);

        let chat =
            encode_request(ApiProtocol::OpenaiChatCompletions, &canonical).expect("encode chat");
        assert_eq!(chat["messages"][0]["role"], "system");
        assert_eq!(
            chat["messages"][0]["content"],
            "Base instruction.\nUse repository rules."
        );
        assert_eq!(chat["messages"][1]["role"], "user");
    }

    #[test]
    fn responses_native_tools_are_preserved_for_responses_upstreams() {
        let namespace_tool = json!({
            "type": "namespace",
            "name": "multi_agent_v1",
            "description": "Agent collaboration tools",
            "tools": [{
                "type": "function",
                "name": "spawn_agent",
                "parameters": {"type": "object", "properties": {}}
            }]
        });
        let web_search_tool = json!({
            "type": "web_search",
            "external_web_access": true
        });
        let request = json!({
            "model": "source-model",
            "input": "Hello",
            "tool_choice": "auto",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {"type": "object", "properties": {}}
                },
                namespace_tool.clone(),
                web_search_tool.clone()
            ]
        });

        let canonical =
            decode_request(ApiProtocol::OpenaiResponses, &request).expect("decode responses");
        assert_eq!(canonical.tools.len(), 1);
        assert_eq!(
            canonical.responses_native_tools,
            vec![namespace_tool.clone(), web_search_tool.clone()]
        );

        let encoded =
            encode_request(ApiProtocol::OpenaiResponses, &canonical).expect("encode responses");
        let encoded_tools = encoded["tools"].as_array().expect("responses tools");
        assert_eq!(encoded_tools.len(), 3);
        assert_eq!(encoded_tools[1], namespace_tool);
        assert_eq!(encoded_tools[2], web_search_tool);
        assert_eq!(encoded["tool_choice"], "auto");
    }

    #[test]
    fn responses_native_tools_degrade_safely_for_chat_upstreams() {
        let request = json!({
            "model": "source-model",
            "instructions": "Keep repository edits focused.",
            "input": "Fix the tests.",
            "tool_choice": "auto",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {"type": "object", "properties": {}}
                },
                {
                    "type": "namespace",
                    "name": "multi_agent_v1",
                    "tools": []
                },
                {
                    "type": "web_search",
                    "external_web_access": true
                }
            ]
        });

        let canonical =
            decode_request(ApiProtocol::OpenaiResponses, &request).expect("decode responses");
        let encoded =
            encode_request(ApiProtocol::OpenaiChatCompletions, &canonical).expect("encode chat");

        let encoded_tools = encoded["tools"].as_array().expect("chat tools");
        assert_eq!(encoded_tools.len(), 1);
        assert_eq!(encoded_tools[0]["type"], "function");
        assert_eq!(encoded_tools[0]["function"]["name"], "exec_command");
        assert_eq!(encoded["tool_choice"], "auto");
        let system = encoded["messages"][0]["content"]
            .as_str()
            .expect("system message");
        assert!(system.contains("Keep repository edits focused."));
        assert!(system.contains("namespace, web_search"));
        assert!(system.contains("Use only the declared function tools."));
    }

    #[test]
    fn explicitly_selected_responses_native_tool_is_not_silently_dropped() {
        let request = json!({
            "model": "source-model",
            "input": "Search the web.",
            "tool_choice": {"type": "web_search"},
            "tools": [{
                "type": "web_search",
                "external_web_access": true
            }]
        });
        let canonical =
            decode_request(ApiProtocol::OpenaiResponses, &request).expect("decode responses");

        let error = encode_request(ApiProtocol::OpenaiChatCompletions, &canonical)
            .expect_err("must reject an explicitly selected native tool");
        assert_eq!(error.code, "unsupported_capability");
        assert!(error.message.contains("web_search"));
        assert!(error.message.contains("OpenAI Chat Completions"));
    }

    #[test]
    fn required_tool_choice_rejects_when_only_native_tools_remain() {
        let request = json!({
            "model": "source-model",
            "input": "Use a tool.",
            "tool_choice": "required",
            "tools": [{
                "type": "web_search",
                "external_web_access": true
            }]
        });
        let canonical =
            decode_request(ApiProtocol::OpenaiResponses, &request).expect("decode responses");

        let error = encode_request(ApiProtocol::OpenaiChatCompletions, &canonical)
            .expect_err("required cannot be satisfied");
        assert_eq!(error.code, "unsupported_capability");
        assert!(error.message.contains("没有可转换的 function Tool"));
    }
}
