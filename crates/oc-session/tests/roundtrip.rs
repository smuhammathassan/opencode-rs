//! Golden round-trip tests: structs must serialize AND deserialize to/from the
//! exact reference zod JSON shapes.
use oc_session::v1;
use oc_session::v2;
use serde_json::json;

#[test]
fn v1_text_part_round_trip() {
    let json = json!({
        "id": "prt_abc",
        "sessionID": "ses_1",
        "messageID": "msg_abc",
        "type": "text",
        "text": "hello",
        "synthetic": true
    });
    let part: v1::Part = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&part).unwrap(), json);
}

#[test]
fn v1_tool_part_round_trip() {
    let json = json!({
        "id": "prt_t",
        "sessionID": "ses_1",
        "messageID": "msg_1",
        "type": "tool",
        "callID": "call_1",
        "tool": "bash",
        "state": {
            "status": "completed",
            "input": { "cmd": "ls" },
            "output": "done",
            "title": "Bash",
            "metadata": {},
            "time": { "start": 1, "end": 2 }
        }
    });
    let part: v1::Part = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&part).unwrap(), json);
}

#[test]
fn v1_user_message_round_trip() {
    let json = json!({
        "id": "msg_1",
        "sessionID": "ses_1",
        "role": "user",
        "time": { "created": 1000 },
        "agent": "primary",
        "model": { "providerID": "openai", "modelID": "gpt-4o" }
    });
    let info: v1::Info = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&info).unwrap(), json);
}

#[test]
fn v1_session_info_round_trip() {
    let json = json!({
        "id": "ses_1",
        "slug": "abc",
        "projectID": "prj_1",
        "directory": "/work",
        "title": "My session",
        "version": "v1.18.13",
        "time": { "created": 1000, "updated": 2000 },
        "cost": 0.0,
        "tokens": {
            "input": 0.0,
            "output": 0.0,
            "reasoning": 0.0,
            "cache": { "read": 0.0, "write": 0.0 }
        }
    });
    let info: v1::SessionInfo = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&info).unwrap(), json);
}

#[test]
fn v2_message_round_trip() {
    let json = json!({
        "id": "msg_2",
        "type": "user",
        "text": "hello",
        "time": { "created": 1000 }
    });
    let message: v2::Message = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&message).unwrap(), json);
}

#[test]
fn v2_assistant_message_round_trip() {
    let json = json!({
        "id": "msg_3",
        "type": "assistant",
        "agent": "primary",
        "model": { "id": "gpt-4o", "providerID": "openai" },
        "content": [
            { "type": "text", "id": "t1", "text": "hi" }
        ],
        "time": { "created": 1000 }
    });
    let message: v2::Message = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&message).unwrap(), json);
}

#[test]
fn v2_compaction_message_round_trip() {
    let json = json!({
        "type": "compaction",
        "reason": "auto",
        "summary": "summary text",
        "recent": "recent text",
        "id": "msg_4",
        "time": { "created": 1000 }
    });
    let message: v2::Message = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&message).unwrap(), json);
}

#[test]
fn message_part_round_trip() {
    let json = json!({
        "type": "tool-invocation",
        "toolInvocation": {
            "state": "call",
            "toolCallId": "call_1",
            "toolName": "bash",
            "args": { "cmd": "ls" }
        }
    });
    let part: oc_session::message::MessagePart = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&part).unwrap(), json);
}
