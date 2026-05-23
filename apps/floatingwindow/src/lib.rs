//! Node.js NAPI module — FloatingWindowService
//!
//! Build as a .node addon with:
//!   cargo build --lib --release --features napi-addon
//!
//! Then copy/rename:
//!   cp target/release/floatingwindow.dll floatingwindow.node   (Windows)
//!   cp target/release/libfloatingwindow.so floatingwindow.node (Linux)
//!   cp target/release/libfloatingwindow.dylib floatingwindow.node (macOS)
//!
//! Electron usage:
//!   const { FloatingWindowService } = require('./floatingwindow.node');
//!   const svc = new FloatingWindowService();
//!   svc.spawn('/path/to/floatingwindow.exe');
//!   svc.send(JSON.stringify({ type: 'openWindow', window: { id: 'mixer', kind: 'mixer', title: 'Mixer' } }));
//!   const msgs = svc.pollMessages(); // call periodically (setInterval 16ms)
//!   svc.stop();

#![allow(clippy::new_without_default)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Sender};
use napi::bindgen_prelude::*;
use napi_derive::napi;

// ── internal state ────────────────────────────────────────────────────────────

struct Inner {
    child: Option<Child>,
    stdin_tx: Option<Sender<String>>,
    inbox: Arc<Mutex<Vec<String>>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            child: None,
            stdin_tx: None,
            inbox: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn is_running(&self) -> bool {
        self.child.is_some()
    }

    fn kill(&mut self) {
        // Closing stdin_tx causes the writer thread to exit, which closes stdin,
        // which signals the floatingwindow process to shut down cleanly.
        self.stdin_tx = None;
        if let Some(mut c) = self.child.take() {
            // Give it a moment then forcibly kill
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let _ = c.kill();
                let _ = c.wait();
            });
        }
    }
}

// ── NAPI class ────────────────────────────────────────────────────────────────

#[napi(js_name = "FloatingWindowService")]
pub struct FloatingWindowService {
    inner: Arc<Mutex<Inner>>,
}

#[napi]
impl FloatingWindowService {
    /// Create a new service instance. Does not spawn the process yet.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::new())),
        }
    }

    /// Spawn the floatingwindow binary at `binary_path`.
    /// Returns an error if already running or if the binary is not found.
    #[napi]
    pub fn spawn(&self, binary_path: String) -> napi::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

        if inner.is_running() {
            return Err(Error::new(
                Status::GenericFailure,
                "FloatingWindowService is already running",
            ));
        }

        let mut child = Command::new(&binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("Failed to spawn '{}': {}", binary_path, e),
                )
            })?;

        let child_stdin = child.stdin.take().expect("stdin piped");
        let child_stdout = child.stdout.take().expect("stdout piped");

        // Channel for messages → stdin writer thread
        let (tx, rx) = unbounded::<String>();

        // Writer thread: receives JSON strings → writes to child stdin
        std::thread::Builder::new()
            .name("fw-stdin".into())
            .spawn(move || {
                let mut stdin = child_stdin;
                while let Ok(msg) = rx.recv() {
                    if writeln!(stdin, "{}", msg).is_err() {
                        break;
                    }
                    let _ = stdin.flush();
                }
            })
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

        // Reader thread: reads child stdout → pushes to inbox
        let inbox = Arc::clone(&inner.inbox);
        std::thread::Builder::new()
            .name("fw-stdout".into())
            .spawn(move || {
                let reader = BufReader::new(child_stdout);
                for line in reader.lines() {
                    match line {
                        Ok(l) if !l.trim().is_empty() => {
                            inbox.lock().unwrap().push(l);
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

        inner.child = Some(child);
        inner.stdin_tx = Some(tx);

        Ok(())
    }

    /// Send a JSON message to the floatingwindow process (written to its stdin).
    #[napi]
    pub fn send(&self, json: String) -> napi::Result<()> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

        if let Some(tx) = &inner.stdin_tx {
            tx.send(json)
                .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
        }
        Ok(())
    }

    /// Drain and return all pending JSON messages from the floatingwindow process.
    /// Call this at ~60 fps (setInterval 16ms) in the Electron main process.
    #[napi]
    pub fn poll_messages(&self) -> napi::Result<Vec<String>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

        let mut inbox = inner
            .inbox
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

        Ok(inbox.drain(..).collect())
    }

    /// Gracefully stop the floatingwindow process.
    #[napi]
    pub fn stop(&self) -> napi::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
        inner.kill();
        Ok(())
    }

    /// Returns true if the floatingwindow process is currently running.
    #[napi]
    pub fn is_running(&self) -> napi::Result<bool> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
        Ok(inner.is_running())
    }
}
