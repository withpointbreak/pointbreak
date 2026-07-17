use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use clap::{Args, ValueEnum};

mod api;
mod cache;
mod server;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum StartupOutputFormat {
    Text,
    Json,
}

/// Open the local, read-only Review web UI over the resolved store (the Work stage)
///
/// `pointbreak inspect` starts a loopback-only web server over the repository's
/// resolved Pointbreak store — the same store every other command reads and
/// writes, whichever storage tier the repository resolves (`pointbreak store
/// paths` reports it). It renders the event timeline, captured revisions,
/// supersession threads, and annotated diffs, and it never executes commands
/// or writes to the store.
#[derive(Debug, Args)]
pub(super) struct InspectArgs {
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Loopback IP address to bind the inspector server to.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind the inspector server to.
    #[arg(long, default_value_t = 7878)]
    port: u16,

    /// Open the inspector in the default browser after the server starts.
    #[arg(long)]
    open: bool,

    /// Serve only the authenticated API, without the browser inspector shell.
    #[arg(long)]
    api_only: bool,

    /// Startup output encoding.
    #[arg(long, value_enum, default_value_t = StartupOutputFormat::Text)]
    format: StartupOutputFormat,
}

pub(super) fn run(
    args: InspectArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let span = tracing::info_span!("shore.inspect");
    let _entered = span.enter();
    tracing::debug!(command = "inspect", "command_start");

    let ip: IpAddr = args
        .host
        .parse()
        .map_err(|_| format!("invalid --host value: {}", args.host))?;
    if !ip.is_loopback() {
        return Err(format!("--host must be a loopback IP address: {ip}").into());
    }
    validate_flag_compatibility(args.api_only, args.open)?;
    let addr = SocketAddr::new(ip, args.port);
    server::serve(
        addr,
        args.repo,
        args.open,
        args.api_only,
        args.format,
        stdout,
    )
}

fn validate_flag_compatibility(
    api_only: bool,
    open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if api_only && open {
        return Err("--open cannot be used with --api-only".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_flag_compatibility;

    #[test]
    fn open_depends_only_on_the_served_surface() {
        assert!(validate_flag_compatibility(false, false).is_ok());
        assert!(validate_flag_compatibility(false, true).is_ok());
        assert!(validate_flag_compatibility(true, false).is_ok());
        assert!(validate_flag_compatibility(true, true).is_err());
    }
}
