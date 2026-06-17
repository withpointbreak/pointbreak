use std::path::PathBuf;

use crate::error::{Result, ShoreError};

/// Environment variable naming the running agent's endpoint (a Unix socket path,
/// or — on Windows — a named-pipe path when the user overrides the default).
const SSH_AUTH_SOCK_ENV: &str = "SSH_AUTH_SOCK";
#[cfg(windows)]
const DEFAULT_OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// Where to reach the agent. Resolved purely from env (testable without I/O),
/// then opened by `connect_agent`.
#[derive(Clone, Debug, Eq, PartialEq)]
enum AgentTarget {
    #[cfg(unix)]
    UnixSocket(PathBuf),
    #[cfg(windows)]
    NamedPipe(PathBuf),
}

/// A connected, platform-abstracted agent stream. `Read + Write`; the wire codec
/// carries the agent protocol over it. On Unix it wraps a `UnixStream`; on Windows
/// it wraps the named-pipe `File`.
pub(crate) struct AgentStream {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    inner: std::fs::File,
}

impl std::io::Read for AgentStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::Write for AgentStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Connect to the running ssh-agent. A missing/unset endpoint or a refused
/// connection is a typed `Err` (the resolve layer degrades it to an unsigned
/// write with a named diagnostic) — never a panic.
pub(crate) fn connect_agent() -> Result<AgentStream> {
    let target = resolve_agent_target(std::env::var_os(SSH_AUTH_SOCK_ENV).map(PathBuf::from))?;
    open_target(target)
}

/// Pure resolution seam (env value in → target out). Kept I/O-free for testing.
fn resolve_agent_target(auth_sock: Option<PathBuf>) -> Result<AgentTarget> {
    #[cfg(unix)]
    {
        let path = auth_sock.ok_or_else(|| {
            ShoreError::Message("no ssh-agent endpoint: SSH_AUTH_SOCK is not set".to_owned())
        })?;
        Ok(AgentTarget::UnixSocket(path))
    }
    #[cfg(windows)]
    {
        // An unset variable falls back to the OpenSSH default pipe (it has a
        // well-known name, unlike a Unix socket path).
        let path = auth_sock.unwrap_or_else(|| PathBuf::from(DEFAULT_OPENSSH_PIPE));
        Ok(AgentTarget::NamedPipe(path))
    }
}

#[cfg(unix)]
fn open_target(target: AgentTarget) -> Result<AgentStream> {
    let AgentTarget::UnixSocket(path) = target;
    let inner = std::os::unix::net::UnixStream::connect(&path).map_err(|error| {
        ShoreError::Message(format!("connect ssh-agent at {}: {error}", path.display()))
    })?;
    Ok(AgentStream { inner })
}

#[cfg(windows)]
fn open_target(target: AgentTarget) -> Result<AgentStream> {
    let AgentTarget::NamedPipe(path) = target;
    // Default transport: open the named pipe as a byte stream via std::fs. The
    // SSH-agent framing is explicitly length-prefixed, so the client does not rely
    // on message-mode pipe semantics. A native-API opener is a possible future
    // contingency, not used here.
    let inner = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| {
            ShoreError::Message(format!(
                "open ssh-agent named pipe {}: {error}",
                path.display()
            ))
        })?;
    Ok(AgentStream { inner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unset_auth_sock_is_a_typed_error_not_a_panic() {
        // Pure seam: None models an unset SSH_AUTH_SOCK. On Unix the resolver
        // returns a typed Err the resolve layer degrades on — never a panic.
        // (On Windows None falls back to the default pipe; see the windows test.)
        #[cfg(unix)]
        {
            let result = resolve_agent_target(None);
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(err.contains("SSH_AUTH_SOCK") || err.to_lowercase().contains("agent"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_target_is_the_auth_sock_path_verbatim() {
        let target = resolve_agent_target(Some(PathBuf::from("/tmp/agent.sock"))).unwrap();
        assert_eq!(
            target,
            AgentTarget::UnixSocket(PathBuf::from("/tmp/agent.sock"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_auth_sock_when_set() {
        let target = resolve_agent_target(Some(PathBuf::from(r"\\.\pipe\my-agent"))).unwrap();
        assert_eq!(
            target,
            AgentTarget::NamedPipe(PathBuf::from(r"\\.\pipe\my-agent"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_defaults_to_openssh_named_pipe_when_unset() {
        // On Windows an unset SSH_AUTH_SOCK falls back to the OpenSSH default pipe
        // (NOT an error — the default pipe is the conventional location).
        let target = resolve_agent_target(None).unwrap();
        assert_eq!(
            target,
            AgentTarget::NamedPipe(PathBuf::from(r"\\.\pipe\openssh-ssh-agent"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn round_trips_a_framed_message_over_a_real_unix_pair() {
        // Hermetic real-socket smoke: a socketpair stands in for the agent
        // connection so we prove a std stream is a working Read + Write WITHOUT a
        // live agent or $SSH_AUTH_SOCK. This only proves bytes flow over the
        // std stream; the codec is proven against the fake in-process agent.
        use std::io::{Read as _, Write as _};
        let (mut client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();

        let request = b"DSSEv1 framed test bytes";
        client.write_all(request).unwrap();
        let mut buf = vec![0u8; request.len()];
        server.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, request);
    }
}
