//! Throwaway micro-benchmark for the render path (not committed to CI).
//! Fills ~a screen via the shell, then times `render()` and `rgba_to_image()`.
//!
//!   cargo run --release --example bench_render

use std::time::{Duration, Instant};

use slint_terminal::{slint_glue, Terminal};

fn main() {
    let (cols, rows) = (80usize, 24usize);
    let mut term = Terminal::new(cols, rows, 20.0, None).expect("terminal");

    // Paint a full screen of mixed text + a bit of color, then let it settle.
    term.feed_input(
        b"printf '\\033[0m'; for i in $(seq 1 24); do \
          printf '%02d The quick brown \\033[31mfox\\033[0m jumps over the \\033[32mlazy\\033[0m dog 0123\\n' $i; \
          done\n",
    );
    std::thread::sleep(Duration::from_millis(600));
    let _ = term.take_dirty();

    let (rgba0, w, h) = term.render();
    let bytes = rgba0.len();
    println!("grid {cols}x{rows}  buffer {w}x{h} = {} KiB", bytes / 1024);

    // Time render() alone.
    let iters = 3000;
    let t0 = Instant::now();
    let mut acc = 0u64;
    for _ in 0..iters {
        let (rgba, _, _) = term.render();
        acc = acc.wrapping_add(rgba[0] as u64); // defeat dead-code elim
    }
    let d = t0.elapsed();
    report("render()", d, iters, bytes);

    // Time render() + rgba_to_image() (the per-frame host cost).
    let t1 = Instant::now();
    for _ in 0..iters {
        let (rgba, w, h) = term.render();
        let img = slint_glue::rgba_to_image(rgba, w, h);
        acc = acc.wrapping_add(img.size().width as u64);
    }
    let d1 = t1.elapsed();
    report("render()+to_image", d1, iters, bytes);

    std::hint::black_box(acc);
}

fn report(label: &str, d: Duration, iters: u32, bytes: usize) {
    let per = d / iters;
    let fps = 1.0 / per.as_secs_f64();
    let mibps = (bytes as f64 * iters as f64) / d.as_secs_f64() / (1024.0 * 1024.0);
    println!("{label:20} {per:>10.2?}/frame  ({fps:>7.0} fps, {mibps:>6.0} MiB/s buffer)");
}
