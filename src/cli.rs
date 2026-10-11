use gtk4::glib;
use std::io::Write;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq)]
pub enum CliAction {
    Continue,
    Exit(glib::ExitCode),
}

pub fn handle_cli_args<I, T, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> CliAction
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
    W: Write,
    E: Write,
{
    let mut args = args.into_iter();
    if let Some(arg) = args.next() {
        match arg.as_ref() {
            "-v" | "-V" | "--version" => {
                let _ = writeln!(stdout, "bubblegum {}", VERSION);
                CliAction::Exit(glib::ExitCode::SUCCESS)
            }
            "-h" | "--help" => {
                let _ = writeln!(
                    stdout,
                    "Bubblegum - Unified package manager GUI\n\nUsage: bubblegum [OPTIONS]\n\nOptions:\n  -v, -V, --version  Print version information\n  -h, --help         Print help"
                );
                CliAction::Exit(glib::ExitCode::SUCCESS)
            }
            unknown => {
                let _ = writeln!(
                    stderr,
                    "error: unexpected argument '{unknown}'\n\nUsage: bubblegum [OPTIONS]\n\nFor more information, try '--help'."
                );
                CliAction::Exit(glib::ExitCode::FAILURE)
            }
        }
    } else {
        CliAction::Continue
    }
}

pub fn run_cli_from<I, T, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> Option<glib::ExitCode>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
    W: Write,
    E: Write,
{
    match handle_cli_args(args, stdout, stderr) {
        CliAction::Continue => None,
        CliAction::Exit(code) => Some(code),
    }
}

pub fn run_cli() -> Option<glib::ExitCode> {
    run_cli_from(
        std::env::args().skip(1),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_version_flag_short_v() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let action = handle_cli_args(["-v"], &mut stdout, &mut stderr);

        assert_eq!(action, CliAction::Exit(glib::ExitCode::SUCCESS));
        let out = String::from_utf8(stdout).expect("valid utf8");
        assert_eq!(out, format!("bubblegum {}\n", VERSION));
        assert!(stderr.is_empty());
    }

    #[test]
    fn test_cli_version_flag_capital_v() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let action = handle_cli_args(["-V"], &mut stdout, &mut stderr);

        assert_eq!(action, CliAction::Exit(glib::ExitCode::SUCCESS));
        let out = String::from_utf8(stdout).expect("valid utf8");
        assert_eq!(out, format!("bubblegum {}\n", VERSION));
        assert!(stderr.is_empty());
    }

    #[test]
    fn test_cli_version_flag_long() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let action = handle_cli_args(["--version"], &mut stdout, &mut stderr);

        assert_eq!(action, CliAction::Exit(glib::ExitCode::SUCCESS));
        let out = String::from_utf8(stdout).expect("valid utf8");
        assert_eq!(out, format!("bubblegum {}\n", VERSION));
        assert!(stderr.is_empty());
    }

    #[test]
    fn test_cli_help_flags() {
        for flag in ["-h", "--help"] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let action = handle_cli_args([flag], &mut stdout, &mut stderr);

            assert_eq!(action, CliAction::Exit(glib::ExitCode::SUCCESS));
            let out = String::from_utf8(stdout).expect("valid utf8");
            assert!(out.contains("Usage: bubblegum"));
            assert!(out.contains("-v, -V, --version"));
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn test_cli_empty_args_continues() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let action = handle_cli_args(Vec::<String>::new(), &mut stdout, &mut stderr);

        assert_eq!(action, CliAction::Continue);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn test_cli_unknown_arg_exits_failure() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let action = handle_cli_args(["--unknown-option"], &mut stdout, &mut stderr);

        assert_eq!(action, CliAction::Exit(glib::ExitCode::FAILURE));
        assert!(stdout.is_empty());
        let err = String::from_utf8(stderr).expect("valid utf8");
        assert!(err.contains("unexpected argument"));
    }

    #[test]
    fn test_run_cli_with_empty_env_args() {
        // Calling run_cli directly executes with ambient env args, ensuring it is callable without panic
        let _ = run_cli();
    }

    #[test]
    fn test_run_cli_from_variants() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_cli_from(["-v"], &mut stdout, &mut stderr),
            Some(glib::ExitCode::SUCCESS)
        );

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_cli_from(Vec::<&str>::new(), &mut stdout, &mut stderr),
            None
        );
    }
}
