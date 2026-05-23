/// SphereDirectAudioEngine — native Rust audio backend for Futureboard Studio.
///
/// # IPC protocol (stdio bridge)
///
/// The Electron main process spawns this binary with `--ipc-stdio`.
/// Communication is newline-delimited JSON on stdin/stdout.
///
/// ## Request (Main → Engine)
/// ```json
/// { "id": "rpc_1", "method": "getStatus", "params": {} }
/// ```
///
/// ## Reply (Engine → Main)
/// ```json
/// { "id": "rpc_1", "result": { "running": true, "version": "0.1.0", ... } }
/// ```
/// ```json
/// { "id": "rpc_1", "error": "Device open failed: permission denied" }
/// ```
///
/// ## Push event (Engine → Main, no id)
/// ```json
/// { "event": "meters", "data": { "master": { "left": 0.12, "right": 0.11 }, "tracks": {}, "timestamp": 1234567890 } }
/// ```
///
/// ## Supported methods
/// - getStatus        → SphereAudioStatus
/// - getVersion       → String
/// - listInputDevices → Vec<AudioDeviceInfo>
/// - listOutputDevices → Vec<AudioDeviceInfo>
/// - openDevice       → ()
/// - closeDevice      → ()
/// - start            → ()
/// - stop             → ()
/// - setTransportState { playing?, positionSeconds?, loop?, loopStart?, loopEnd? } → ()
/// - getTransportState → { playing, positionSeconds }
/// - updateTrackParam { trackId, paramId, value } → ()
/// - updateInsertParam { trackId, insertId, paramId, value } → ()
/// - loadProject      (EngineProjectSnapshot) → ()
/// - updateClip       { clipId, patch } → ()
/// - getMeters        → SphereMeterSnapshot
use std::io::{self, BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let stdio_mode = args.iter().any(|a| a == "--ipc-stdio");

    if stdio_mode {
        run_ipc_stdio();
    } else {
        eprintln!("[SphereAudio] No mode specified. Use --ipc-stdio for Electron integration.");
        std::process::exit(1);
    }
}

fn run_ipc_stdio() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Send a ready event so the host knows we started cleanly.
    let ready = r#"{"event":"ready","data":{"version":"0.1.0-stub"}}"#;
    writeln!(out, "{}", ready).ok();
    out.flush().ok();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[SphereAudio] JSON parse error: {}", e);
                continue;
            }
        };

        let id = msg["id"].as_str().unwrap_or("").to_string();
        let method = msg["method"].as_str().unwrap_or("").to_string();

        let result = dispatch(&method, &msg["params"]);

        let reply = match result {
            Ok(v) => format!(r#"{{"id":"{}","result":{}}}"#, id, v),
            Err(e) => format!(r#"{{"id":"{}","error":"{}"}}"#, id, e.replace('"', "'")),
        };

        writeln!(out, "{}", reply).ok();
        out.flush().ok();
    }
}

fn dispatch(method: &str, params: &serde_json::Value) -> Result<String, String> {
    match method {
        "getStatus" => Ok(serde_json::json!({
            "running":      true,
            "version":      "0.1.0-stub",
            "sampleRate":   44100,
            "bufferSize":   256,
            "inputDevice":  null,
            "outputDevice": null,
            "cpuLoad":      0.0,
            "xrunCount":    0
        })
        .to_string()),

        "getVersion" => Ok(r#""0.1.0-stub""#.to_string()),

        "listInputDevices" | "listOutputDevices" => Ok(r#"[]"#.to_string()),

        "openDevice" | "closeDevice" => Ok(r#"null"#.to_string()),

        "start" | "stop" => Ok(r#"null"#.to_string()),

        "setTransportState" => {
            // TODO: apply transport state to audio graph
            let _ = params;
            Ok(r#"null"#.to_string())
        }

        "getTransportState" => Ok(serde_json::json!({
            "playing": false,
            "positionSeconds": 0.0
        })
        .to_string()),

        "updateTrackParam" => {
            // TODO: forward to realtime audio graph
            let _ = params;
            Ok(r#"null"#.to_string())
        }

        "updateInsertParam" => {
            let _ = params;
            Ok(r#"null"#.to_string())
        }

        "loadProject" => {
            // TODO: build audio graph from snapshot
            let _ = params;
            Ok(r#"null"#.to_string())
        }

        "updateClip" => {
            let _ = params;
            Ok(r#"null"#.to_string())
        }

        "getMeters" => Ok(serde_json::json!({
            "tracks": {},
            "master": { "left": 0.0, "right": 0.0 },
            "timestamp": 0
        })
        .to_string()),

        other => Err(format!("Unknown method: {}", other)),
    }
}
