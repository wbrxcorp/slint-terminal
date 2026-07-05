//! Standalone terminal: runs a shell in a Slint window. Doubles as the dev
//! harness for the library (same core code path the consumer app uses).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;
use slint_terminal::{slint_glue, Terminal};

slint::include_modules!();

const COLS: usize = 80;
const ROWS: usize = 24;
const FONT_PX: f32 = 16.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terminal = Terminal::new(COLS, ROWS, FONT_PX, None)?;
    let (px_w, px_h) = terminal.pixel_size();
    let terminal = Rc::new(RefCell::new(terminal));

    let ui = MainWindow::new()?;
    ui.window()
        .set_size(slint::PhysicalSize::new(px_w, px_h));

    // Quit the app when the shell exits (e.g. the user typed `exit`).
    terminal
        .borrow_mut()
        .set_on_exit(|_code| {
            let _ = slint::quit_event_loop();
        });

    // Keyboard -> PTY.
    {
        let terminal = terminal.clone();
        ui.on_key(move |text, ctrl, alt| {
            if let Some(bytes) = slint_glue::key_to_bytes(text.as_str(), ctrl, alt) {
                terminal.borrow().feed_input(&bytes);
            }
        });
    }

    // Paint the first frame immediately so the window isn't blank.
    {
        let mut term = terminal.borrow_mut();
        let (rgba, w, h) = term.render();
        ui.set_frame(slint_glue::rgba_to_image(rgba, w, h));
    }

    // ~60 fps tick: adopt window resizes, repaint when dirty, quit on shell exit.
    let timer = slint::Timer::default();
    {
        let terminal = terminal.clone();
        let ui_weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                let mut term = terminal.borrow_mut();

                // Match the grid to the current window size.
                let size = ui.window().size();
                let (cols, rows) = term.cells_for_pixels(size.width, size.height);
                let _ = term.resize(cols, rows);

                if term.take_dirty() {
                    let (rgba, w, h) = term.render();
                    ui.set_frame(slint_glue::rgba_to_image(rgba, w, h));
                }

                // Fire the on-exit callback once if the shell has exited.
                term.poll();
            },
        );
    }

    ui.run()?;
    Ok(())
}
