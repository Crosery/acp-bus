use acp_core::adapter::AdapterConfig;
use acp_core::client::{AcpClient, BusEvent, ClientEvent};
use std::collections::HashMap;
use tokio::sync::mpsc;

fn mock_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test binary name
    path.pop(); // remove "deps"
    path.push("acp-mock");
    path.to_string_lossy().to_string()
}

fn mock_adapter(env: HashMap<String, String>) -> AdapterConfig {
    AdapterConfig {
        name: "mock".to_string(),
        description: "mock agent".to_string(),
        cmd: mock_bin(),
        args: vec![],
        env,
        terminal: false,
        auth_method: None,
        auth_api_key: None,
        system_prompt: None,
        disallowed_tools: vec![],
        socket_path: None,
    }
}

#[tokio::test]
async fn test_happy_path() {
    let adapter = mock_adapter(HashMap::new());
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let (client, _rx) = AcpClient::start(adapter, cwd, None, "test".to_string())
        .await
        .unwrap();
    assert!(client.alive);
    assert_eq!(client.session_id.as_deref(), Some("mock-session-1"));

    let stop_reason = client.prompt("hello").await.unwrap();
    assert_eq!(stop_reason, "end_turn");
}

#[tokio::test]
async fn test_streaming_events() {
    let mut env = HashMap::new();
    env.insert("MOCK_STREAM_CHUNKS".into(), "3".into());
    env.insert("MOCK_STREAM_DELAY_MS".into(), "5".into());

    let adapter = mock_adapter(env);
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let (client, mut rx) = AcpClient::start(adapter, cwd, None, "test".to_string())
        .await
        .unwrap();

    // Spawn a task to collect SessionUpdate events
    let collector = tokio::spawn(async move {
        let mut updates = vec![];
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await {
                Ok(Some(ClientEvent::SessionUpdate(v))) => updates.push(v),
                _ => break,
            }
        }
        updates
    });

    let stop_reason = client.prompt("test streaming").await.unwrap();
    assert_eq!(stop_reason, "end_turn");

    let updates = collector.await.unwrap();
    assert_eq!(updates.len(), 3);
}

#[tokio::test]
async fn test_init_failure() {
    let mut env = HashMap::new();
    env.insert("MOCK_INIT_FAIL".into(), "1".into());

    let adapter = mock_adapter(env);
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let result = AcpClient::start(adapter, cwd, None, "test".to_string()).await;
    assert!(result.is_err());
    let err_msg = result.err().unwrap().to_string();
    assert!(err_msg.contains("mock init failure"), "got: {err_msg}");
}

#[tokio::test]
async fn test_prompt_failure() {
    let mut env = HashMap::new();
    env.insert("MOCK_PROMPT_FAIL".into(), "1".into());

    let adapter = mock_adapter(env);
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let (client, _rx) = AcpClient::start(adapter, cwd, None, "test".to_string())
        .await
        .unwrap();
    let result = client.prompt("fail please").await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("mock prompt failure"), "got: {err_msg}");
}

#[tokio::test]
async fn test_agent_crash() {
    let mut env = HashMap::new();
    env.insert("MOCK_PROMPT_EXIT".into(), "1".into());

    let adapter = mock_adapter(env);
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let (client, mut rx) = AcpClient::start(adapter, cwd, None, "test".to_string())
        .await
        .unwrap();

    // Prompt will fail because mock exits
    let result = client.prompt("crash please").await;
    assert!(result.is_err());

    // Should receive an Exited event
    let ev = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(ev) = rx.recv().await {
            if matches!(ev, ClientEvent::Exited { .. }) {
                return ev;
            }
        }
        panic!("no Exited event received");
    })
    .await
    .unwrap();

    assert!(matches!(ev, ClientEvent::Exited { .. }));
}

#[tokio::test]
async fn test_bus_send_message() {
    let mut env = HashMap::new();
    env.insert("MOCK_BUS_SEND".into(), "1".into());

    let adapter = mock_adapter(env);
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let (bus_tx, mut bus_rx) = mpsc::unbounded_channel::<BusEvent>();

    let (client, _rx) = AcpClient::start(adapter, cwd, Some(bus_tx), "test-agent".to_string())
        .await
        .unwrap();

    // Prompt triggers mock to send bus/send_message
    let stop_reason = client.prompt("trigger bus send").await.unwrap();
    assert_eq!(stop_reason, "end_turn");

    // Should receive a BusEvent::SendMessage
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), bus_rx.recv())
        .await
        .unwrap()
        .expect("should receive bus event");

    match event {
        BusEvent::SendMessage {
            from_agent,
            to_agent,
            content,
            reply_tx,
        } => {
            assert_eq!(from_agent, "test-agent");
            assert_eq!(to_agent, "main");
            assert_eq!(content, "hello from mock via bus");
            let _ = reply_tx.send(acp_core::client::BusSendResult {
                message_id: Some(1),
                delivered: true,
                error: None,
            });
        }
        _ => panic!("expected SendMessage, got {:?}", "ListAgents"),
    }
}
