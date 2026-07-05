//! PTY + shell process via `portable-pty`.

use std::io::{Read, Write};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    /// Open a PTY and spawn `program` (or the user's `$SHELL` when `None`).
    pub fn spawn(
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
        program: Option<&str>,
    ) -> Result<Self, String> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width,
                pixel_height,
            })
            .map_err(|e| format!("openpty: {e}"))?;

        let shell = program
            .map(String::from)
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_string());

        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn shell: {e}"))?;
        // Drop the slave end so the shell owns the only slave fd; otherwise the
        // reader never sees EOF when the shell exits.
        drop(pair.slave);

        Ok(Self {
            master: pair.master,
            child,
        })
    }

    /// A clone of the master reader (PTY output). Move to the reader thread.
    pub fn reader(&self) -> Result<Box<dyn Read + Send>, String> {
        self.master
            .try_clone_reader()
            .map_err(|e| format!("clone reader: {e}"))
    }

    /// The master writer (input to the shell). Take once.
    pub fn writer(&self) -> Result<Box<dyn Write + Send>, String> {
        self.master
            .take_writer()
            .map_err(|e| format!("take writer: {e}"))
    }

    pub fn resize(
        &self,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width,
                pixel_height,
            })
            .map_err(|e| format!("resize pty: {e}"))
    }

    /// Non-blocking check whether the shell has exited.
    pub fn try_wait(&mut self) -> Option<u32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.exit_code()),
            _ => None,
        }
    }

    /// Terminate the shell process (best effort). Safe to call after it has
    /// already exited.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}
