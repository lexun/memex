//! Simple test to validate Claude CLI subprocess spawning
//!
//! Run with: cargo run --example spawn_test -p cortex
//!
//! This tests two approaches:
//! 1. Simple -p mode with JSON output
//! 2. Stream-json bidirectional communication

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Test 1: Simple -p mode with JSON output ===\n");
    test_simple_mode().await?;

    println!("\n=== Test 2: Stream-json bidirectional mode ===\n");
    test_stream_mode().await?;

    Ok(())
}

/// Test simple --print mode with JSON output
async fn test_simple_mode() -> anyhow::Result<()> {
    println!("Spawning: claude -p 'What is 2+2? Reply with just the number.' --output-format json");

    let output = Command::new("claude")
        .arg("-p")
        .arg("What is 2+2? Reply with just the number.")
        .arg("--output-format")
        .arg("json")
        .arg("--dangerously-skip-permissions")
        .output()
        .await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Output: {}", stdout);

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(result) = json.get("result") {
                println!("Result field: {}", result);
            }
        }
        println!("\nSimple mode: SUCCESS");
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("Error: {}", stderr);
        println!("\nSimple mode: FAILED");
    }

    Ok(())
}

/// Test stream-json bidirectional mode
async fn test_stream_mode() -> anyhow::Result<()> {
    // Based on error messages, try different message formats
    let formats_to_try = vec![
        // Format 1: Simple text with type "user"
        r#"{"type":"user","message":{"role":"user","content":"What is 2+2?"}}"#,
        // Format 2: Direct content
        r#"{"type":"user","content":"What is 2+2?"}"#,
    ];

    for (i, format) in formats_to_try.iter().enumerate() {
        println!("\nTrying format {}: {}", i + 1, format);

        let mut child = Command::new("claude")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--print")
            .arg("--verbose")
            .arg("--dangerously-skip-permissions")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Read stderr in background
        let stderr_handle = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let mut errors = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                errors.push(line);
            }
            errors
        });

        // Send message
        stdin.write_all(format.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        // Read responses with short timeout
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let read_result = timeout(Duration::from_secs(5), async {
            let mut responses = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                responses.push(line.clone());
                // Check for completion
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                    if json.get("type") == Some(&serde_json::json!("result")) {
                        break;
                    }
                }
            }
            responses
        })
        .await;

        let _ = child.kill().await;

        // Check results
        let stderr_output = stderr_handle.await.unwrap_or_default();

        match read_result {
            Ok(responses) if !responses.is_empty() => {
                println!("Got {} responses:", responses.len());
                for r in &responses {
                    println!("  {}", r);
                }
                println!("\nFormat {} worked!", i + 1);
                return Ok(());
            }
            Ok(_) => {
                println!("No responses received");
                if !stderr_output.is_empty() {
                    println!("Stderr: {:?}", stderr_output);
                }
            }
            Err(_) => {
                println!("Timeout");
                if !stderr_output.is_empty() {
                    println!("Stderr: {:?}", stderr_output);
                }
            }
        }
    }

    println!("\nStream mode: All formats failed - may need more investigation");
    Ok(())
}
