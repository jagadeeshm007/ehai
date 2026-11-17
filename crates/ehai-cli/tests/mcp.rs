//! MCP protocol test: spawn `ehai mcp`, speak JSON-RPC over stdio, and check that
//! malformed input never takes the server down (spec §13).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    _home: tempfile::TempDir,
}

impl Server {
    fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            "role = \"pm\"\n[access]\nproject_admin = true\n",
        )
        .unwrap();
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("ehai"))
            .arg("mcp")
            .env("EHAI_HOME", home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            lines,
            _home: home,
        }
    }

    fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn send(&mut self, msg: Value) {
        self.send_raw(&msg.to_string());
    }

    /// Next response carrying `id` (skipping notifications).
    fn response(&self, id: i64) -> Value {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .expect("server replied in time");
            let v: Value = serde_json::from_str(&line).expect("server writes JSON lines only");
            if v["id"] == id {
                return v;
            }
        }
    }

    fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.response(id)
    }
}

impl Server {
    /// Close stdin like a harness does at session end; the server must exit cleanly.
    fn shutdown(mut self) -> std::process::ExitStatus {
        drop(self.stdin.take());
        for _ in 0..250 {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("server did not exit after stdin closed");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn speaks_mcp_over_stdio_and_survives_bad_input() {
    let mut s = Server::start();
    let init = s.call(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "ehai", "{init}");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(init["result"]["capabilities"]["resources"].is_object());
    s.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let tools = s.call(2, "tools/list", json!({}));
    let ping = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ping")
        .expect("ping tool listed");
    assert!(ping["inputSchema"].is_object());

    let pong = s.call(3, "tools/call", json!({"name": "ping", "arguments": {}}));
    assert_eq!(pong["result"]["structuredContent"]["ok"], true, "{pong}");
    assert_eq!(
        pong["result"]["structuredContent"]["version"],
        env!("CARGO_PKG_VERSION")
    );

    let res = s.call(4, "resources/list", json!({}));
    assert_eq!(res["result"]["resources"][0]["uri"], "ehai://prefs");
    let prefs = s.call(5, "resources/read", json!({"uri": "ehai://prefs"}));
    let text = prefs["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("role = \"pm\""), "{text}");

    // Bad input: garbage line, unknown tool, unknown resource, wrong params.
    s.send_raw("this is not json");
    let unknown_tool = s.call(6, "tools/call", json!({"name": "nope", "arguments": {}}));
    assert!(unknown_tool.get("error").is_some() || unknown_tool["result"]["isError"] == true);
    let unknown_res = s.call(7, "resources/read", json!({"uri": "ehai://nope"}));
    assert!(unknown_res.get("error").is_some(), "{unknown_res}");
    let bad_params = s.call(8, "tools/call", json!({"arguments": 42}));
    assert!(bad_params.get("error").is_some(), "{bad_params}");

    // Still alive and answering.
    let again = s.call(9, "tools/call", json!({"name": "ping", "arguments": {}}));
    assert_eq!(again["result"]["structuredContent"]["ok"], true);

    assert!(
        s.shutdown().success(),
        "clean exit when the client disconnects"
    );
}
