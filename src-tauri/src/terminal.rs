//! Integrated terminal backed by a real PTY. Output streams to the UI as
//! `pty-output` events; input comes back through `pty_write`.

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use tauri::{AppHandle, Emitter};

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct Terminals {
    map: Mutex<HashMap<String, Pty>>,
}

#[derive(Serialize, Clone)]
struct Output {
    id: String,
    data: String,
}

#[derive(Serialize, Clone)]
struct Exit {
    id: String,
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").map(|_| "powershell.exe".to_string()).unwrap_or_else(|_| "powershell.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

impl Terminals {
    pub fn spawn(&self, app: AppHandle, cwd: &str, cols: u16, rows: u16) -> Result<String> {
        let pty = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let mut cmd = CommandBuilder::new(default_shell());
        if std::path::Path::new(cwd).is_dir() {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let child = pty.slave.spawn_command(cmd)?;
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader()?;
        let writer = pty.master.take_writer()?;
        let id = uuid::Uuid::new_v4().to_string();

        let id2 = id.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        // Emit only complete UTF-8; keep a split character for the next read.
                        let valid = match std::str::from_utf8(&pending) {
                            Ok(_) => pending.len(),
                            Err(e) if e.error_len().is_none() => e.valid_up_to(),
                            Err(_) => pending.len(),
                        };
                        let chunk: Vec<u8> = pending.drain(..valid).collect();
                        let data = String::from_utf8_lossy(&chunk).to_string();
                        let _ = app.emit("pty-output", Output { id: id2.clone(), data });
                    }
                }
            }
            let _ = app.emit("pty-exit", Exit { id: id2 });
        });

        self.map.lock().insert(id.clone(), Pty { master: pty.master, writer, child });
        Ok(id)
    }

    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        let mut map = self.map.lock();
        let p = map.get_mut(id).ok_or_else(|| anyhow!("terminal closed"))?;
        p.writer.write_all(data.as_bytes())?;
        p.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let map = self.map.lock();
        let p = map.get(id).ok_or_else(|| anyhow!("terminal closed"))?;
        p.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    pub fn kill(&self, id: &str) {
        if let Some(mut p) = self.map.lock().remove(id) {
            let _ = p.child.kill();
        }
    }
}
