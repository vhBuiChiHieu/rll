use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(rll::run_stdio())
}
