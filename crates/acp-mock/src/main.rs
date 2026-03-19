use acp_protocol::{
    decode, encode_error, encode_notification, encode_request, encode_response, next_id,
};
use std::env;
use tokio::io::{AsyncBufReadExt, BufReader};

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str) -> bool {
    env::var(key).ok().map(|v| v == "1").unwrap_or(false)
}

fn env_str(key: &str, default: &str) -> String {
    env::var(key).ok().unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() {
    let stream_chunks = env_u64("MOCK_STREAM_CHUNKS", 0);
    let stream_delay = env_u64("MOCK_STREAM_DELAY_MS", 10);
    let response_text = env_str("MOCK_RESPONSE_TEXT", "mock response");
    let init_fail = env_bool("MOCK_INIT_FAIL");
    let prompt_fail = env_bool("MOCK_PROMPT_FAIL");
    let prompt_exit = env_bool("MOCK_PROMPT_EXIT");
    let bus_send = env_bool("MOCK_BUS_SEND");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let msg = match decode(&line) {
            Some(m) => m,
            None => continue,
        };

        if !msg.is_request() {
            continue;
        }

        let id = msg.id.as_ref().unwrap().clone();
        let method = msg.method.as_deref().unwrap_or("");

        match method {
            "initialize" => {
                if init_fail {
                    let out = encode_error(&id, -32000, "mock init failure");
                    println!("{out}");
                } else {
                    let out = encode_response(
                        &id,
                        serde_json::json!({
                            "protocolVersion": 1,
                            "agentCapabilities": {}
                        }),
                    );
                    println!("{out}");
                }
            }
            "session/new" => {
                let out = encode_response(
                    &id,
                    serde_json::json!({
                        "sessionId": "mock-session-1"
                    }),
                );
                println!("{out}");
            }
            "session/prompt" => {
                if prompt_exit {
                    std::process::exit(1);
                }
                if prompt_fail {
                    let out = encode_error(&id, -32000, "mock prompt failure");
                    println!("{out}");
                    continue;
                }

                // If bus_send is enabled, send a bus/send_message reverse request
                if bus_send {
                    let bus_id = next_id();
                    let bus_req = encode_request(
                        bus_id,
                        "bus/send_message",
                        serde_json::json!({
                            "to": "main",
                            "content": "hello from mock via bus"
                        }),
                    );
                    println!("{bus_req}");
                    // Read response to our bus request
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            // Just consume the response
                        }
                    }
                }

                for i in 0..stream_chunks {
                    let notif = encode_notification(
                        "session/update",
                        serde_json::json!({
                            "update": {
                                "sessionUpdate": "content",
                                "content": [{"type": "text", "text": format!("{response_text} chunk {i}")}]
                            }
                        }),
                    );
                    println!("{notif}");
                    if stream_delay > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(stream_delay)).await;
                    }
                }

                let out = encode_response(
                    &id,
                    serde_json::json!({
                        "stopReason": "end_turn"
                    }),
                );
                println!("{out}");
            }
            "session/cancel" => {
                let out = encode_response(&id, serde_json::json!({}));
                println!("{out}");
            }
            _ => {
                let out = encode_error(&id, -32601, &format!("method not found: {method}"));
                println!("{out}");
            }
        }
    }
}
