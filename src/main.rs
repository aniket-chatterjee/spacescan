use std::process::ExitCode;

use clap::Parser;

use spacescan::cli::Cli;
use spacescan::constants;
use spacescan::runner;

#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Switch the Windows console to UTF-8 so box-drawing and bar glyphs render.
#[cfg(windows)]
fn enable_utf8_console() {
    extern "system" {
        fn SetConsoleOutputCP(code_page: u32) -> i32;
        fn SetConsoleCP(code_page: u32) -> i32;
    }
    // Safety: simple FFI calls with no pointer arguments; failures are ignored.
    unsafe {
        SetConsoleOutputCP(constants::scan::WINDOWS_UTF8_CODE_PAGE);
        SetConsoleCP(constants::scan::WINDOWS_UTF8_CODE_PAGE);
    }
}

fn main() -> ExitCode {
    #[cfg(windows)]
    enable_utf8_console();

    let cli = Cli::parse();
    runner::run(cli)
}
