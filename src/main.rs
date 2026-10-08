use std::process::ExitCode;

mod cli;
mod cli_tracing;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    if let Some(mode) = arguments.next() {
        if mode == "__owned-change-v1" {
            return private_change_command(arguments.collect());
        }
        if mode == "__owned-enrollment-v1" {
            return private_enrollment_command(arguments.collect());
        }
        if mode == "__owned-audit-v1" {
            return private_audit_command(arguments.collect());
        }
        if mode == "__owned-append-v1" {
            return private_append_command(arguments.collect());
        }
        if mode == "__owned-inspect-v1" {
            return cli::inspect::owned::shell_main(arguments.collect());
        }
        if mode == "__inspect-data-v1" {
            return cli::inspect::owned::child_main(arguments.collect());
        }
        if mode == "__owned-data-v1" {
            return private_data_command(arguments.collect());
        }
    }
    cli::run_main()
}

#[cfg(target_os = "macos")]
fn private_data_command(arguments: Vec<std::ffi::OsString>) -> ExitCode {
    use std::io::Read as _;

    use pointbreak::session::{
        OwnedDataRequest, OwnedProcessFailure, enter_owned_data_command, run_owned_data_request,
    };
    fn terminal(_: OwnedProcessFailure) -> ! {
        // SAFETY: this branch owns the disposable process before CLI setup.
        unsafe { libc::_exit(90) }
    }
    let result = (|| -> pointbreak::error::Result<_> {
        if !arguments.is_empty() {
            return Err(pointbreak::error::ShoreError::Message(
                "private data command has no positional arguments".into(),
            ));
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|error| pointbreak::error::ShoreError::Message(error.to_string()))?;
        if bytes.len() > 65_536 {
            return Err(pointbreak::error::ShoreError::Message(
                "private data request limit".into(),
            ));
        }
        let input: OwnedDataRequest = serde_json::from_slice(&bytes)?;
        // SAFETY: this private binary branch precedes CLI parsing, tracing,
        // async runtimes and services. It owns the sole data thread to exit.
        let owner = unsafe { enter_owned_data_command(terminal) }?;
        run_owned_data_request(
            &owner,
            input,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    })();
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn private_data_command(_: Vec<std::ffi::OsString>) -> ExitCode {
    eprintln!("owned_process_required; native data entry unsupported on this platform");
    ExitCode::from(2)
}

#[cfg(any(target_os = "macos", windows))]
fn private_enrollment_command(arguments: Vec<std::ffi::OsString>) -> ExitCode {
    use std::io::Read as _;

    use pointbreak::session::{OwnedProcessBootstrap, OwnedProcessFailure, run_disposable_owner};
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        version: u32,
        root: std::path::PathBuf,
        request_id: String,
        writer: pointbreak::session::event::Writer,
        occurred_at: String,
        signing_key: std::path::PathBuf,
        selected_only: bool,
    }
    fn terminal(_: OwnedProcessFailure) -> ! {
        // SAFETY: this branch owns the whole command; no Rust/atexit unwinding.
        unsafe { libc::_exit(90) }
    }
    let result = (|| -> pointbreak::error::Result<_> {
        if !arguments.is_empty() {
            return Err(pointbreak::error::ShoreError::Message(
                "private bootstrap has no positional arguments".into(),
            ));
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| pointbreak::error::ShoreError::Message(e.to_string()))?;
        if bytes.len() > 65536 {
            return Err(pointbreak::error::ShoreError::Message(
                "private bootstrap exceeds limit".into(),
            ));
        }
        let input: Input = serde_json::from_slice(&bytes)?;
        if input.version != 1
            || !input.signing_key.is_absolute()
            || !input.root.is_absolute()
            || input.request_id.len() != 64
            || !input
                .request_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(pointbreak::error::ShoreError::Message(
                "invalid private bootstrap version, request or absolute paths".into(),
            ));
        }
        let signer = pointbreak::keys::load_signer_from_path(&input.signing_key)?;
        let bootstrap = OwnedProcessBootstrap {
            root: input.root,
            request_id: input.request_id,
            writer: input.writer,
            occurred_at: input.occurred_at,
            signer: Box::new(signer),
            selected_only: input.selected_only,
        };
        // SAFETY: selected before CLI, tracing, workers or native store creation.
        // The entire process is this one command, with an immediate exit policy.
        unsafe { run_disposable_owner(bootstrap, terminal) }
    })();
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string(&value).expect("owned exit serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn private_enrollment_command(_: Vec<std::ffi::OsString>) -> ExitCode {
    eprintln!("owned_process_required; native enrollment unsupported on this platform");
    ExitCode::from(2)
}

#[cfg(any(target_os = "macos", windows))]
fn private_append_command(arguments: Vec<std::ffi::OsString>) -> ExitCode {
    use std::io::Read as _;

    use pointbreak::session::{
        OwnedAppendBootstrap, OwnedAppendOperation, OwnedProcessFailure, run_disposable_append,
    };
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        version: u32,
        root: std::path::PathBuf,
        request_id: String,
        enrollment_actor: String,
        operation: OwnedAppendOperation,
    }
    fn terminal(_: OwnedProcessFailure) -> ! {
        // SAFETY: this branch owns the whole command before any CLI/worker setup.
        unsafe { libc::_exit(90) }
    }
    let result = (|| -> pointbreak::error::Result<_> {
        if !arguments.is_empty() {
            return Err(pointbreak::error::ShoreError::Message(
                "private append has no positional arguments".into(),
            ));
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(262145)
            .read_to_end(&mut bytes)
            .map_err(|e| pointbreak::error::ShoreError::Message(e.to_string()))?;
        if bytes.len() > 262144 {
            return Err(pointbreak::error::ShoreError::Message(
                "private append input exceeds diagnostic limit".into(),
            ));
        }
        let input: Input = serde_json::from_slice(&bytes)?;
        if input.version != 1 {
            return Err(pointbreak::error::ShoreError::Message(
                "invalid private append version".into(),
            ));
        }
        // SAFETY: sole audited production call site; whole-command process,
        // no sibling data work, no embedding host, immediate terminal callback.
        unsafe {
            run_disposable_append(
                OwnedAppendBootstrap {
                    root: input.root,
                    request_id: input.request_id,
                    enrollment_actor: input.enrollment_actor,
                    operation: input.operation,
                },
                terminal,
            )
        }
    })();
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string(&value).expect("owned result serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
fn private_append_command(_: Vec<std::ffi::OsString>) -> ExitCode {
    eprintln!("owned_process_required; native append unsupported on this platform");
    ExitCode::from(2)
}

#[cfg(any(target_os = "macos", windows))]
fn private_audit_command(arguments: Vec<std::ffi::OsString>) -> ExitCode {
    use std::io::Read as _;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use pointbreak::session::{
        AuditLimits, OwnedAuditBootstrap, OwnedProcessFailure, run_disposable_audit,
    };
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        version: u32,
        root: std::path::PathBuf,
        request_id: String,
        enrollment_actor: String,
        audit_request: String,
        #[serde(default)]
        limits: AuditLimits,
    }
    fn terminal(_: OwnedProcessFailure) -> ! {
        // SAFETY: this private branch owns the whole command before CLI setup.
        unsafe { libc::_exit(90) }
    }
    let started_at = std::time::Instant::now();
    let result = (|| -> pointbreak::error::Result<_> {
        if !arguments.is_empty() {
            return Err(pointbreak::error::ShoreError::Message(
                "private audit has no positional arguments".into(),
            ));
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|e| pointbreak::error::ShoreError::Message(e.to_string()))?;
        if bytes.len() > 65_536 {
            return Err(pointbreak::error::ShoreError::Message(
                "private audit bootstrap limit".into(),
            ));
        }
        let input: Input = serde_json::from_slice(&bytes)?;
        if input.version != 1 {
            return Err(pointbreak::error::ShoreError::Message(
                "unsupported private audit version".into(),
            ));
        }
        // SAFETY: sole audited production caller; owns the disposable process,
        // has no sibling work, and terminates without unwinding on uncertainty.
        unsafe {
            run_disposable_audit(
                OwnedAuditBootstrap {
                    root: input.root,
                    request_id: input.request_id,
                    enrollment_actor: input.enrollment_actor,
                    audit_request: input.audit_request,
                    limits: input.limits,
                    cancellation: Arc::new(AtomicBool::new(false)),
                    bootstrap_bytes: bytes.len() as u64,
                    started_at,
                },
                terminal,
            )
        }
    })();
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string(&value).expect("audit result serializes")
            );
            if value.finalized {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(3)
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
fn private_audit_command(_: Vec<std::ffi::OsString>) -> ExitCode {
    eprintln!("owned_process_required; native audit unsupported on this platform");
    ExitCode::from(2)
}

#[cfg(target_os = "macos")]
fn private_change_command(arguments: Vec<std::ffi::OsString>) -> ExitCode {
    fn terminal(_: pointbreak::session::OwnedProcessFailure) -> ! {
        // SAFETY: this branch owns the disposable process; never unwind.
        unsafe { libc::_exit(90) }
    }
    if arguments.len() < 4 {
        eprintln!(
            "private Change entry requires request, enroll-empty/resume, then change create arguments"
        );
        return ExitCode::from(2);
    }
    let Some(request) = arguments[0].to_str() else {
        return ExitCode::from(2);
    };
    let mode = arguments[1].to_str();
    if !matches!(
        mode,
        Some("enroll-empty" | "resume" | "resume-cut-after-presence")
    ) {
        return ExitCode::from(2);
    }
    // SAFETY: selection precedes parsing and any worker/native setup. The only
    // dispatch below is the audited synchronous Change-create handler.
    let owner = unsafe {
        pointbreak::session::enter_owned_change_command(
            request.to_owned(),
            mode == Some("enroll-empty"),
            mode == Some("resume-cut-after-presence"),
            terminal,
        )
    };
    match owner {
        Ok(_owner) => cli::run_owned_change(arguments.into_iter().skip(2).collect()),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn private_change_command(_: Vec<std::ffi::OsString>) -> ExitCode {
    eprintln!("private Change consumer is admitted only for macOS development");
    ExitCode::from(2)
}
