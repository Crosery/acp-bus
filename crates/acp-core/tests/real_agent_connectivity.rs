//! Real agent connectivity tests.
//! These tests connect to actual agent binaries (claude-agent-acp, codex-acp, gemini).
//! They only do the handshake (initialize + session/new) to verify connectivity,
//! without sending prompts (to avoid API costs).
//!
//! Run with: cargo test -p acp-core --test real_agent_connectivity -- --ignored
//! (These are #[ignore] by default since they require installed binaries + auth)

use acp_core::adapter::{self, AdapterOpts};
use acp_core::client::AcpClient;

async fn test_handshake(adapter_name: &str) {
    let opts = AdapterOpts {
        bus_mode: true,
        is_main: false,
        agent_name: Some("connectivity-test".into()),
        channel_id: Some("test-channel".into()),
        cwd: Some("/tmp".into()),
    };

    let config = adapter::get(adapter_name, &opts)
        .unwrap_or_else(|e| panic!("adapter '{adapter_name}' config failed: {e}"));

    // Check binary exists
    let bin_exists = std::process::Command::new("which")
        .arg(&config.cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !bin_exists {
        eprintln!("SKIP: {} binary '{}' not found", adapter_name, config.cmd);
        return;
    }

    eprintln!("Testing {adapter_name} ({})...", config.cmd);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        AcpClient::start(config, "/tmp".into(), None, adapter_name.to_string()),
    )
    .await;

    match result {
        Ok(Ok((client, _rx))) => {
            eprintln!(
                "  ✓ {adapter_name} connected — session_id={:?}",
                client.session_id
            );
            assert!(
                client.alive,
                "{adapter_name} should be alive after handshake"
            );
            assert!(
                client.session_id.is_some(),
                "{adapter_name} should have session_id"
            );
        }
        Ok(Err(e)) => {
            panic!("  ✗ {adapter_name} handshake failed: {e}");
        }
        Err(_) => {
            panic!("  ✗ {adapter_name} handshake timed out (30s)");
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_claude_connectivity() {
    test_handshake("claude").await;
}

#[tokio::test]
#[ignore]
async fn test_codex_connectivity() {
    test_handshake("codex").await;
}

#[tokio::test]
#[ignore]
async fn test_gemini_connectivity() {
    test_handshake("gemini").await;
}

/// Run all connectivity tests in sequence
#[tokio::test]
#[ignore]
async fn test_all_adapters_connectivity() {
    for name in adapter::list() {
        test_handshake(name).await;
    }
}
