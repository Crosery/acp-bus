use acp_protocol::encode_notification;

pub fn channel_message(
    channel_id: &str,
    from: &str,
    content: &str,
    timestamp: i64,
    gap: Option<i64>,
) -> String {
    let mut msg = serde_json::json!({
        "channel_id": channel_id,
        "message": {
            "from": from,
            "content": content,
            "timestamp": timestamp,
        },
    });
    if let Some(gap) = gap {
        msg["gap"] = serde_json::json!(gap);
    }
    encode_notification("channel/message", msg)
}

pub fn agent_state_changed(channel_id: &str, agents: &[serde_json::Value]) -> String {
    encode_notification(
        "agent/state_changed",
        serde_json::json!({
            "channel_id": channel_id,
            "agents": agents,
        }),
    )
}

pub fn channel_closed(channel_id: &str) -> String {
    encode_notification(
        "channel/closed",
        serde_json::json!({ "channel_id": channel_id }),
    )
}
