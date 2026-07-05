//! # slint-terminal
//!
//! A reusable terminal emulator built as a **framework-agnostic core** plus an
//! optional thin **`slint` glue** feature.
//!
//! The core ([`Terminal`]) owns a PTY + shell, an `alacritty_terminal` grid,
//! and a fontdue-based rasterizer, and exposes the terminal as an RGBA pixel
//! buffer. It has no dependency on Slint, so a consuming app can keep its own
//! slint version free of coupling (see the crate README for the rationale).
//!
//! The `slint` feature (on by default) adds [`slint_glue`]: RGBA → `slint::Image`
//! conversion and `slint::platform::Key` → input-byte mapping.

mod font;
mod pty;
mod render;
mod terminal;

#[cfg(feature = "slint")]
pub mod slint_glue;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use font::FontRaster;
use pty::Pty;
use render::Renderer;
use terminal::{GridSize, TerminalState};

/// A running terminal: PTY + shell + grid + rasterizer.
///
/// Rendering is single-threaded (call [`Terminal::render`] from your UI/render
/// thread). PTY I/O runs on background threads; grid mutation is guarded by an
/// internal mutex.
pub struct Terminal {
    state: Arc<Mutex<TerminalState>>,
    renderer: Renderer,
    pty: Pty,
    input_tx: Sender<Vec<u8>>,
    dirty: Arc<AtomicBool>,
    eof: Arc<AtomicBool>,
    cols: usize,
    rows: usize,
    cell_w: usize,
    cell_h: usize,
    _reader: JoinHandle<()>,
    _writer: JoinHandle<()>,
    exit_code: Option<u32>,
    on_exit: Option<Box<dyn FnMut(u32)>>,
    exit_notified: bool,
}

impl Terminal {
    /// Create a terminal of `cols`x`rows` cells using a `font_px` monospace
    /// font, spawning `program` (or `$SHELL` when `None`).
    pub fn new(
        cols: usize,
        rows: usize,
        font_px: f32,
        program: Option<&str>,
    ) -> Result<Self, String> {
        let font = FontRaster::new(font_px)?;
        let renderer = Renderer::new(font);
        let (cell_w, cell_h) = renderer.cell_size();
        let (px_w, px_h) = renderer.pixel_size(cols, rows);

        let pty = Pty::spawn(
            cols as u16,
            rows as u16,
            px_w as u16,
            px_h as u16,
            program,
        )?;

        // Single channel carries both keyboard input and terminal replies
        // (device queries etc.) to the shell.
        let (input_tx, input_rx) = channel::<Vec<u8>>();

        let state = Arc::new(Mutex::new(TerminalState::new(
            GridSize {
                columns: cols,
                screen_lines: rows,
            },
            input_tx.clone(),
        )));

        let dirty = Arc::new(AtomicBool::new(true));
        let eof = Arc::new(AtomicBool::new(false));

        // Writer thread: drains the input channel into the PTY.
        let mut writer = pty.writer()?;
        let writer_thread = std::thread::spawn(move || {
            while let Ok(bytes) = input_rx.recv() {
                if writer.write_all(&bytes).is_err() || writer.flush().is_err() {
                    break;
                }
            }
        });

        // Reader thread: feeds PTY output into the grid.
        let mut reader = pty.reader()?;
        let reader_state = state.clone();
        let reader_dirty = dirty.clone();
        let reader_eof = eof.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Ok(mut st) = reader_state.lock() {
                            st.advance(&buf[..n]);
                        }
                        reader_dirty.store(true, Ordering::Relaxed);
                    }
                }
            }
            reader_eof.store(true, Ordering::Relaxed);
        });

        Ok(Self {
            state,
            renderer,
            pty,
            input_tx,
            dirty,
            eof,
            cols,
            rows,
            cell_w,
            cell_h,
            _reader: reader_thread,
            _writer: writer_thread,
            exit_code: None,
            on_exit: None,
            exit_notified: false,
        })
    }

    /// Send bytes to the shell (already-encoded key input, paste, etc.).
    pub fn feed_input(&self, bytes: &[u8]) {
        let _ = self.input_tx.send(bytes.to_vec());
    }

    /// True (and cleared) if the grid changed since the last call — cheap gate
    /// for skipping redundant renders.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    /// Render the current grid to RGBA. Returns `(rgba, width, height)`.
    pub fn render(&mut self) -> (&[u8], u32, u32) {
        let st = self.state.lock().expect("terminal state poisoned");
        self.renderer.render(&st)
    }

    /// Resize the grid (in cells) and the PTY to match.
    pub fn resize(&mut self, cols: usize, rows: usize) -> Result<(), String> {
        if cols == self.cols && rows == self.rows {
            return Ok(());
        }
        self.cols = cols;
        self.rows = rows;
        let (px_w, px_h) = self.renderer.pixel_size(cols, rows);
        if let Ok(mut st) = self.state.lock() {
            st.resize(GridSize {
                columns: cols,
                screen_lines: rows,
            });
        }
        self.pty
            .resize(cols as u16, rows as u16, px_w as u16, px_h as u16)?;
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Register a callback fired once, from [`Terminal::poll`], when the shell
    /// exits (e.g. the user typed `exit`). The argument is the exit code.
    ///
    /// Fires on whatever thread calls `poll` — for a Slint host that is the UI
    /// thread. Do **not** drop this `Terminal` from inside the callback (it is
    /// borrowed while `poll` runs); instead signal the host to tear it down
    /// after `poll` returns (see the crate README embedding notes).
    pub fn set_on_exit(&mut self, cb: impl FnMut(u32) + 'static) {
        self.on_exit = Some(Box::new(cb));
    }

    /// Drive one housekeeping step: detect shell exit and fire the [`set_on_exit`]
    /// callback exactly once. Call this each UI tick (alongside rendering).
    ///
    /// [`set_on_exit`]: Terminal::set_on_exit
    pub fn poll(&mut self) {
        if self.exit_notified {
            return;
        }
        if let Some(code) = self.exit_code() {
            self.exit_notified = true;
            if let Some(mut cb) = self.on_exit.take() {
                cb(code);
            }
        }
    }

    /// The shell's exit code once it has exited, else `None`. Latches. Pure
    /// getter — for callback-driven exit handling use [`Terminal::poll`].
    pub fn exit_code(&mut self) -> Option<u32> {
        if self.exit_code.is_some() {
            return self.exit_code;
        }
        if let Some(code) = self.pty.try_wait() {
            self.exit_code = Some(code);
        } else if self.eof.load(Ordering::Relaxed) {
            // Reader saw EOF; give the child a chance to report its code.
            self.exit_code = self.pty.try_wait().or(Some(0));
        }
        self.exit_code
    }

    pub fn cell_size(&self) -> (usize, usize) {
        (self.cell_w, self.cell_h)
    }

    pub fn grid_size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    pub fn pixel_size(&self) -> (u32, u32) {
        self.renderer.pixel_size(self.cols, self.rows)
    }

    /// How many whole cells fit into a pixel area — for translating window
    /// sizes into grid dimensions on resize.
    pub fn cells_for_pixels(&self, px_w: u32, px_h: u32) -> (usize, usize) {
        let c = (px_w as usize / self.cell_w).max(1);
        let r = (px_h as usize / self.cell_h).max(1);
        (c, r)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Kill the shell so the reader thread hits EOF and exits; dropping
        // `input_tx` ends the writer thread. Both are detached and finish on
        // their own. This matters when a host tears the terminal down while
        // the shell is still running (e.g. a "Back" button), not just on exit.
        self.pty.kill();
    }
}
