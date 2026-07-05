// The .slint UI is only needed by the standalone binary, which requires the
// "slint" feature. Compile it only then; the framework-agnostic core builds
// without any slint tooling. Build scripts see `--cfg feature="..."`, so this
// gate also keeps the optional slint-build dependency out of the core build.
fn main() {
    #[cfg(feature = "slint")]
    slint_build::compile("ui/main.slint").expect("compile ui/main.slint");
}
