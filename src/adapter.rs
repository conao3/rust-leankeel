use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

use crate::Failure;

pub struct Adapter {
    program: std::ffi::OsString,
    project: Option<PathBuf>,
    options: Map<String, Value>,
    setup: Value,
}

fn die_with_parent(command: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() == 1 {
                    libc::raise(libc::SIGKILL);
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = command;
}

fn module_setup(file: &Path, project: &Path) -> Result<Value, Failure> {
    let file = std::fs::canonicalize(file)
        .map_err(|e| Failure::Input(format!("{}: {e}", file.display())))?;
    let mut command = Command::new("lake");
    command.arg("setup-file").arg(&file).current_dir(project);
    die_with_parent(&mut command);
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::Internal(format!("cannot start lake: {e}")))?;
    if !output.status.success() {
        return Err(Failure::Input(format!(
            "lake setup-file {} failed in {}:\n{}{}",
            file.display(),
            project.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| Failure::Internal(format!("invalid lake setup-file output: {e}")))
}

impl Adapter {
    pub fn new(
        file: Option<&Path>,
        project: Option<PathBuf>,
        options: Map<String, Value>,
    ) -> Result<Adapter, Failure> {
        let program = std::env::var_os("LEANKEEL_ADAPTER").ok_or_else(|| {
            Failure::Internal("LEANKEEL_ADAPTER must name the leankeel-adapter executable".into())
        })?;
        let setup = match (file, &project) {
            (Some(file), Some(project)) => module_setup(file, project)?,
            _ => Value::Null,
        };
        Ok(Adapter {
            program,
            project,
            options,
            setup,
        })
    }

    pub fn run(&self, file_name: &str, source: &str) -> Result<String, Failure> {
        let mut command = match &self.project {
            Some(project) => {
                let mut lake = Command::new("lake");
                lake.arg("env").arg(&self.program).current_dir(project);
                lake
            }
            None => Command::new(&self.program),
        };
        die_with_parent(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Failure::Internal(format!("cannot start the Lean adapter: {e}")))?;
        let request = json!({
            "FileName": file_name,
            "Source": source,
            "Options": self.options,
            "Setup": self.setup,
        });
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let written = writeln!(stdin, "{request}");
        drop(stdin);
        let output = child
            .wait_with_output()
            .map_err(|e| Failure::Internal(format!("the Lean adapter failed: {e}")))?;
        if !output.status.success() || written.is_err() {
            return Err(Failure::Internal(format!(
                "the Lean adapter failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|e| Failure::Internal(format!("invalid adapter output: {e}")))
    }
}
