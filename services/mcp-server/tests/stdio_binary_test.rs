//! Regression tests for the stdio transport, run against the real binary.
//!
//! These guard the invariant that stdout carries **only** JSON-RPC frames: if
//! logging is ever wired back to stdout, MCP clients stop being able to parse
//! the stream.

use std::io::Write;
use std::process::{Command, Stdio};

fn spawn_stdio(env: &[(&str, &str)]) -> std::process::Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_controlplane-mcp"));
    command
        .env("MCP_TRANSPORT", "stdio")
        .env_remove("MCP_ALLOW_ANONYMOUS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    command.spawn().expect("failed to spawn controlplane-mcp")
}

fn send(child: &mut std::process::Child, lines: &[&str]) {
    let stdin = child.stdin.as_mut().expect("stdin available");
    for line in lines {
        writeln!(stdin, "{line}").expect("write to stdin");
    }
}

#[test]
fn stdout_carries_only_json_rpc_frames() {
    let mut child = spawn_stdio(&[
        ("MCP_TOKEN", "test-token"),
        ("MCP_AUTH_TOKENS", "test-token:admin"),
        ("RUST_LOG", "info"),
    ]);

    send(
        &mut child,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            // A notification must produce no response at all.
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ],
    );
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("process output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    assert_eq!(
        lines.len(),
        2,
        "expected exactly two responses (initialize + tools/list), got:\n{stdout}"
    );
    for line in &lines {
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("stdout line is not valid JSON ({e}): {line}"));
        assert_eq!(value["jsonrpc"], "2.0");
    }

    // Logs must have gone to stderr instead.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.trim().is_empty(),
        "expected diagnostics on stderr, but it was empty"
    );
    assert!(
        stdout.contains("\"protocolVersion\""),
        "initialize response missing from stdout"
    );
}

#[test]
fn stdio_without_credentials_fails_closed() {
    // Neither MCP_TOKEN nor MCP_ROLE configured: the transport must refuse to
    // start rather than serving unauthenticated reads.
    let output = Command::new(env!("CARGO_BIN_EXE_controlplane-mcp"))
        .env("MCP_TRANSPORT", "stdio")
        .env_remove("MCP_TOKEN")
        .env_remove("MCP_ROLE")
        .env_remove("MCP_ALLOW_ANONYMOUS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run binary");

    assert!(!output.status.success(), "expected a non-zero exit");
    assert!(output.stdout.is_empty(), "no protocol output expected");
}

#[test]
fn invalid_token_exits_non_zero_without_stdout_output() {
    let mut child = spawn_stdio(&[
        ("MCP_TOKEN", "bogus"),
        ("MCP_AUTH_TOKENS", "test-token:admin"),
    ]);

    send(
        &mut child,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#],
    );
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("process output");
    assert!(
        !output.status.success(),
        "fail-closed: invalid credential must exit non-zero"
    );
    assert!(
        output.stdout.is_empty(),
        "no protocol output should be produced when the credential is invalid"
    );
}

#[test]
fn malformed_line_yields_a_parse_error_and_keeps_running() {
    let mut child = spawn_stdio(&[
        ("MCP_TOKEN", "test-token"),
        ("MCP_AUTH_TOKENS", "test-token:admin"),
    ]);

    send(
        &mut child,
        &[
            "this is not json",
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
        ],
    );
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("process output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "expected parse error then ping response");

    let parse_error: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(parse_error["error"]["code"], -32700);

    let ping: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(ping["id"], 7);
    assert!(ping["error"].is_null());
}
