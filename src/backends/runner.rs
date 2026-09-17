use super::BackendError;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, BackendError>;
    fn run_command(&self, program: &str, args: &[&str]) -> Result<CommandOutput, BackendError>;
    fn run_streaming(
        &self,
        program: &str,
        args: &[&str],
        on_line: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<CommandOutput, BackendError>;
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run_command(&self, program: &str, args: &[&str]) -> Result<CommandOutput, BackendError> {
        let output = match Command::new(program).args(args).output() {
            Ok(out) => out,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(BackendError::Unavailable {
                    program: program.to_string(),
                });
            }
            Err(e) => {
                return Err(BackendError::Io {
                    detail: e.to_string(),
                });
            }
        };

        let status = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(CommandOutput {
            status,
            stdout,
            stderr,
        })
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, BackendError> {
        let out = self.run_command(program, args)?;
        if out.status != 0 {
            Err(BackendError::CommandFailed {
                program: program.to_string(),
                status: out.status,
                stderr: out.stderr,
            })
        } else {
            Ok(out)
        }
    }

    fn run_streaming(
        &self,
        program: &str,
        args: &[&str],
        on_line: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<CommandOutput, BackendError> {
        let mut child = match Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(BackendError::Unavailable {
                    program: program.to_string(),
                });
            }
            Err(e) => {
                return Err(BackendError::Io {
                    detail: e.to_string(),
                });
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();

        std::thread::scope(|s| {
            let t_stdout = s.spawn(|| {
                let mut out_str = String::new();
                if let Some(pipe) = stdout {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(pipe);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            on_line(&l);
                            out_str.push_str(&l);
                            out_str.push('\n');
                        }
                    }
                }
                out_str
            });

            let t_stderr = s.spawn(|| {
                let mut err_str = String::new();
                if let Some(pipe) = stderr {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(pipe);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            on_line(&l);
                            err_str.push_str(&l);
                            err_str.push('\n');
                        }
                    }
                }
                err_str
            });

            stdout_buf = t_stdout.join().unwrap_or_default();
            stderr_buf = t_stderr.join().unwrap_or_default();
        });

        let status = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

        if status != 0 {
            Err(BackendError::CommandFailed {
                program: program.to_string(),
                status,
                stderr: stderr_buf,
            })
        } else {
            Ok(CommandOutput {
                status,
                stdout: stdout_buf,
                stderr: stderr_buf,
            })
        }
    }
}

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
type MockResponses = HashMap<(String, Vec<String>), Result<CommandOutput, BackendError>>;

#[cfg(test)]
#[derive(Default)]
pub struct MockCommandRunner {
    pub responses: Mutex<MockResponses>,
    pub calls: Mutex<Vec<(String, Vec<String>)>>,
}

#[cfg(test)]
impl MockCommandRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_response(
        &self,
        program: &str,
        args: &[&str],
        result: Result<CommandOutput, BackendError>,
    ) {
        let key = (
            program.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        );
        self.responses.lock().unwrap().insert(key, result);
    }

    pub fn get_calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl CommandRunner for MockCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, BackendError> {
        let arg_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.calls
            .lock()
            .unwrap()
            .push((program.to_string(), arg_vec.clone()));

        let key = (program.to_string(), arg_vec);
        if let Some(res) = self.responses.lock().unwrap().get(&key) {
            res.clone()
        } else {
            Err(BackendError::Unavailable {
                program: program.to_string(),
            })
        }
    }

    fn run_command(&self, program: &str, args: &[&str]) -> Result<CommandOutput, BackendError> {
        self.run(program, args)
    }

    fn run_streaming(
        &self,
        program: &str,
        args: &[&str],
        on_line: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<CommandOutput, BackendError> {
        let res = self.run(program, args);
        match res {
            Ok(out) => {
                for line in out.stdout.lines() {
                    on_line(line);
                }
                for line in out.stderr.lines() {
                    on_line(line);
                }
                Ok(out)
            }
            Err(BackendError::CommandFailed {
                program,
                status,
                stderr,
            }) => {
                for line in stderr.lines() {
                    on_line(line);
                }
                Err(BackendError::CommandFailed {
                    program,
                    status,
                    stderr,
                })
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_runner_success() {
        let runner = MockCommandRunner::new();
        runner.set_response(
            "test_cmd",
            &["arg1", "arg2"],
            Ok(CommandOutput {
                status: 0,
                stdout: "output text".to_string(),
                stderr: String::new(),
            }),
        );

        let res = runner.run("test_cmd", &["arg1", "arg2"]).unwrap();
        assert_eq!(res.status, 0);
        assert_eq!(res.stdout, "output text");

        let calls = runner.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "test_cmd");
        assert_eq!(calls[0].1, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_mock_runner_failure() {
        let runner = MockCommandRunner::new();
        runner.set_response(
            "failing_cmd",
            &["--bad"],
            Err(BackendError::CommandFailed {
                program: "failing_cmd".to_string(),
                status: 1,
                stderr: "something went wrong".to_string(),
            }),
        );

        let err = runner.run("failing_cmd", &["--bad"]).unwrap_err();
        match err {
            BackendError::CommandFailed {
                program,
                status,
                stderr,
            } => {
                assert_eq!(program, "failing_cmd");
                assert_eq!(status, 1);
                assert_eq!(stderr, "something went wrong");
            }
            _ => panic!("Expected CommandFailed error"),
        }
    }

    #[test]
    fn test_mock_runner_unavailable() {
        let runner = MockCommandRunner::new();
        let err = runner.run("nonexistent", &[]).unwrap_err();
        assert_eq!(
            err,
            BackendError::Unavailable {
                program: "nonexistent".to_string()
            }
        );
    }

    #[test]
    fn test_mock_runner_streaming_success() {
        let runner = MockCommandRunner::new();
        runner.set_response(
            "stream_cmd",
            &["arg1"],
            Ok(CommandOutput {
                status: 0,
                stdout: "line1\nline2\n".to_string(),
                stderr: "err1\n".to_string(),
            }),
        );

        let lines = std::sync::Mutex::new(Vec::new());
        let res = runner
            .run_streaming("stream_cmd", &["arg1"], &|line| {
                lines.lock().unwrap().push(line.to_string());
            })
            .unwrap();

        assert_eq!(res.status, 0);
        assert_eq!(
            *lines.lock().unwrap(),
            vec!["line1".to_string(), "line2".to_string(), "err1".to_string()]
        );
    }

    #[test]
    fn test_mock_runner_streaming_failure() {
        let runner = MockCommandRunner::new();
        runner.set_response(
            "stream_fail",
            &[],
            Err(BackendError::CommandFailed {
                program: "stream_fail".to_string(),
                status: 2,
                stderr: "fail line 1\nfail line 2".to_string(),
            }),
        );

        let lines = std::sync::Mutex::new(Vec::new());
        let err = runner
            .run_streaming("stream_fail", &[], &|line| {
                lines.lock().unwrap().push(line.to_string());
            })
            .unwrap_err();

        assert!(matches!(err, BackendError::CommandFailed { status: 2, .. }));
        assert_eq!(
            *lines.lock().unwrap(),
            vec!["fail line 1".to_string(), "fail line 2".to_string()]
        );
    }

    #[test]
    fn test_system_runner_streaming_success() {
        let runner = SystemCommandRunner;
        let lines = std::sync::Mutex::new(Vec::new());
        let res = runner
            .run_streaming("sh", &["-c", "echo hello; echo world"], &|line| {
                lines.lock().unwrap().push(line.to_string());
            })
            .unwrap();
        assert_eq!(res.status, 0);
        let collected = lines.lock().unwrap().clone();
        assert!(collected.contains(&"hello".to_string()));
        assert!(collected.contains(&"world".to_string()));
    }

    #[test]
    fn test_system_runner_streaming_failure() {
        let runner = SystemCommandRunner;
        let lines = std::sync::Mutex::new(Vec::new());
        let err = runner
            .run_streaming("sh", &["-c", "echo err_msg >&2; exit 42"], &|line| {
                lines.lock().unwrap().push(line.to_string());
            })
            .unwrap_err();
        match err {
            BackendError::CommandFailed { status, stderr, .. } => {
                assert_eq!(status, 42);
                assert!(stderr.contains("err_msg"));
            }
            _ => panic!("Expected CommandFailed"),
        }
        let collected = lines.lock().unwrap().clone();
        assert!(collected.contains(&"err_msg".to_string()));
    }

    #[test]
    fn test_system_runner_streaming_unavailable() {
        let runner = SystemCommandRunner;
        let lines = std::sync::Mutex::new(Vec::new());
        let err = runner
            .run_streaming("nonexistent_binary_xyz_12345", &[], &|line| {
                lines.lock().unwrap().push(line.to_string());
            })
            .unwrap_err();
        assert_eq!(
            err,
            BackendError::Unavailable {
                program: "nonexistent_binary_xyz_12345".to_string()
            }
        );
    }

    #[test]
    fn test_system_runner_run_success() {
        let runner = SystemCommandRunner;
        let res = runner.run("sh", &["-c", "echo hello_runner"]).unwrap();
        assert_eq!(res.status, 0);
        assert!(res.stdout.contains("hello_runner"));
    }

    #[test]
    fn test_system_runner_run_failure() {
        let runner = SystemCommandRunner;
        let err = runner
            .run("sh", &["-c", "echo failure_detail >&2; exit 17"])
            .unwrap_err();
        match err {
            BackendError::CommandFailed { status, stderr, .. } => {
                assert_eq!(status, 17);
                assert!(stderr.contains("failure_detail"));
            }
            _ => panic!("Expected CommandFailed"),
        }
    }

    #[test]
    fn test_system_runner_run_command_non_zero() {
        let runner = SystemCommandRunner;
        let out = runner
            .run_command("sh", &["-c", "echo out_msg; echo err_msg >&2; exit 1"])
            .unwrap();
        assert_eq!(out.status, 1);
        assert!(out.stdout.contains("out_msg"));
        assert!(out.stderr.contains("err_msg"));
    }
}

