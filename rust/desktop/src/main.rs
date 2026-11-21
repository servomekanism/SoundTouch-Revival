use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("The desktop app is under development. Use the Python CLI for preset management.");
    ExitCode::FAILURE
}
