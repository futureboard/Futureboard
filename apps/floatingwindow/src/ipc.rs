use std::io::{BufRead, BufReader, Write};
use crossbeam_channel::{unbounded, Receiver};

use crate::protocol::{IncomingMessage, OutgoingMessage};

/// Spawns stdin reader and stdout writer threads.
/// Returns a receiver for incoming messages.
/// Outgoing messages are passed via the returned sender.
pub fn spawn_ipc(out_rx: Receiver<OutgoingMessage>) -> Receiver<IncomingMessage> {
    let (in_tx, in_rx) = unbounded::<IncomingMessage>();

    // Reader thread: stdin → IncomingMessage channel
    std::thread::Builder::new()
        .name("ipc-stdin".into())
        .spawn(move || {
            let reader = BufReader::new(std::io::stdin());
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<IncomingMessage>(&line) {
                            Ok(msg) => {
                                if in_tx.send(msg).is_err() {
                                    break; // App is shutting down
                                }
                            }
                            Err(e) => {
                                tracing::warn!("IPC parse error: {} | input: {}", e, line);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stdin read error: {}", e);
                        break;
                    }
                }
            }
            tracing::info!("IPC stdin thread exit");
        })
        .expect("failed to spawn ipc-stdin thread");

    // Writer thread: OutgoingMessage channel → stdout
    std::thread::Builder::new()
        .name("ipc-stdout".into())
        .spawn(move || {
            let mut stdout = std::io::stdout();
            while let Ok(msg) = out_rx.recv() {
                match serde_json::to_string(&msg) {
                    Ok(json) => {
                        if writeln!(stdout, "{}", json).is_err() {
                            break;
                        }
                        let _ = stdout.flush();
                    }
                    Err(e) => {
                        tracing::warn!("IPC serialize error: {}", e);
                    }
                }
            }
            tracing::info!("IPC stdout thread exit");
        })
        .expect("failed to spawn ipc-stdout thread");

    in_rx
}
