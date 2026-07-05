//! Thin wrapper around `alacritty_terminal`'s `Term` + the vte ANSI parser.
//!
//! Bytes read from the PTY are fed through the parser into the grid; the grid
//! is later walked by [`crate::render`] to produce pixels. This module knows
//! nothing about pixels or Slint.

use std::sync::mpsc::Sender;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

/// Grid dimensions handed to `Term`. `alacritty_terminal`'s own `TermSize`
/// lives in its test module, so we provide our own `Dimensions` impl.
#[derive(Clone, Copy)]
pub struct GridSize {
    pub columns: usize,
    pub screen_lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }
    fn screen_lines(&self) -> usize {
        self.screen_lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Event sink for `Term`. The only events we must act on for a usable terminal
/// are the ones that write back to the PTY (device queries, color/size
/// requests); everything else is ignored for the PoC.
pub struct EventProxy {
    /// Sender into the PTY's input (same path as keyboard input).
    to_pty: Sender<Vec<u8>>,
}

impl EventProxy {
    pub fn new(to_pty: Sender<Vec<u8>>) -> Self {
        Self { to_pty }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                let _ = self.to_pty.send(text.into_bytes());
            }
            Event::TextAreaSizeRequest(fmt) => {
                // Report a plausible pixel size; refined callers can override.
                let reply = fmt(alacritty_terminal::event::WindowSize {
                    num_lines: 24,
                    num_cols: 80,
                    cell_width: 1,
                    cell_height: 1,
                });
                let _ = self.to_pty.send(reply.into_bytes());
            }
            _ => {}
        }
    }
}

/// The terminal state machine: grid + parser.
pub struct TerminalState {
    term: Term<EventProxy>,
    parser: Processor,
}

impl TerminalState {
    pub fn new(size: GridSize, to_pty: Sender<Vec<u8>>) -> Self {
        let config = Config::default();
        let term = Term::new(config, &size, EventProxy::new(to_pty));
        Self {
            term,
            parser: Processor::new(),
        }
    }

    /// Feed raw bytes read from the PTY into the parser/grid.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, size: GridSize) {
        self.term.resize(size);
    }

    /// Borrow the underlying `Term` for rendering.
    pub fn term(&self) -> &Term<EventProxy> {
        &self.term
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.term.columns(), self.term.screen_lines())
    }
}
