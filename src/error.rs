pub type Result<T> = std::result::Result<T, ShoreError>;

#[derive(Debug, thiserror::Error)]
pub enum ShoreError {
    #[error("{0}")]
    Message(String),

    #[error(
        "git command failed: {command}\nstatus: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    )]
    GitCommand {
        command: String,
        status: String,
        stdout: String,
        stderr: String,
    },
}
