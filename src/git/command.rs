use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::{Result, ShoreError};

#[derive(Debug)]
pub(crate) struct GitOutput {
    pub stdout: Vec<u8>,
}

pub(crate) fn run_git<I, S>(cwd: &Path, args: I) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let output = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
        .map_err(|error| ShoreError::Message(format!("run git {:?}: {error}", args)))?;

    if !output.status.success() {
        return Err(ShoreError::GitCommand {
            command: format!("{args:?}"),
            status: output.status.to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(GitOutput {
        stdout: output.stdout,
    })
}
