use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LongitudinalProcessCpuClockV1 {
    MachAbsoluteTime { numerator: u32, denominator: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LongitudinalProcessSnapshotV1 {
    pub pid: u32,
    pub user_cpu_nanos: u64,
    pub system_cpu_nanos: u64,
    pub resident_bytes: u64,
    pub cpu_clock: LongitudinalProcessCpuClockV1,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LongitudinalProcessCaptureError {
    #[error("longitudinal process capture is unsupported on this platform")]
    UnsupportedPlatform,
    #[error("mach_timebase_info failed with code {code}")]
    MachTimebaseInfo { code: i32 },
    #[error("invalid Mach timebase {numerator}/{denominator}")]
    InvalidTimebase { numerator: u32, denominator: u32 },
    #[error("process id {pid} cannot be represented by the macOS process API")]
    InvalidProcessId { pid: u32 },
    #[error("proc_pid_rusage failed for pid {pid} with code {code}")]
    ProcessRusage { pid: u32, code: i32 },
    #[error("converted process CPU time exceeds u64 nanoseconds")]
    CpuTimeOverflow,
}

pub fn longitudinal_mach_ticks_to_nanos_v1(
    ticks: u64,
    numerator: u32,
    denominator: u32,
) -> Result<u64, LongitudinalProcessCaptureError> {
    if numerator == 0 || denominator == 0 {
        return Err(LongitudinalProcessCaptureError::InvalidTimebase {
            numerator,
            denominator,
        });
    }
    let nanos = u128::from(ticks)
        .checked_mul(u128::from(numerator))
        .ok_or(LongitudinalProcessCaptureError::CpuTimeOverflow)?
        / u128::from(denominator);
    u64::try_from(nanos).map_err(|_| LongitudinalProcessCaptureError::CpuTimeOverflow)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn mach_timebase_v1() -> Result<(u32, u32), LongitudinalProcessCaptureError> {
    let mut timebase = libc::mach_timebase_info { numer: 0, denom: 0 };
    let result = unsafe { libc::mach_timebase_info(&mut timebase) };
    if result != 0 {
        return Err(LongitudinalProcessCaptureError::MachTimebaseInfo { code: result });
    }
    if timebase.numer == 0 || timebase.denom == 0 {
        return Err(LongitudinalProcessCaptureError::InvalidTimebase {
            numerator: timebase.numer,
            denominator: timebase.denom,
        });
    }
    Ok((timebase.numer, timebase.denom))
}

#[cfg(target_os = "macos")]
fn process_rusage_v2(
    pid: i32,
    public_pid: u32,
) -> Result<libc::rusage_info_v2, LongitudinalProcessCaptureError> {
    use std::mem::MaybeUninit;

    let mut usage = MaybeUninit::<libc::rusage_info_v2>::uninit();
    let rusage_result =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) };
    if rusage_result != 0 {
        return Err(LongitudinalProcessCaptureError::ProcessRusage {
            pid: public_pid,
            code: rusage_result,
        });
    }
    Ok(unsafe { usage.assume_init() })
}

#[cfg(target_os = "macos")]
pub fn capture_longitudinal_process_snapshot_v1(
    pid: u32,
) -> Result<LongitudinalProcessSnapshotV1, LongitudinalProcessCaptureError> {
    let native_pid = i32::try_from(pid)
        .map_err(|_| LongitudinalProcessCaptureError::InvalidProcessId { pid })?;
    let (timebase_numerator, timebase_denominator) = mach_timebase_v1()?;
    let usage = process_rusage_v2(native_pid, pid)?;

    Ok(LongitudinalProcessSnapshotV1 {
        pid,
        user_cpu_nanos: longitudinal_mach_ticks_to_nanos_v1(
            usage.ri_user_time,
            timebase_numerator,
            timebase_denominator,
        )?,
        system_cpu_nanos: longitudinal_mach_ticks_to_nanos_v1(
            usage.ri_system_time,
            timebase_numerator,
            timebase_denominator,
        )?,
        resident_bytes: usage.ri_resident_size,
        cpu_clock: LongitudinalProcessCpuClockV1::MachAbsoluteTime {
            numerator: timebase_numerator,
            denominator: timebase_denominator,
        },
    })
}

#[cfg(not(target_os = "macos"))]
pub fn capture_longitudinal_process_snapshot_v1(
    _pid: u32,
) -> Result<LongitudinalProcessSnapshotV1, LongitudinalProcessCaptureError> {
    Err(LongitudinalProcessCaptureError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longitudinal_process_cpu_converts_exact_mach_ticks() {
        assert_eq!(longitudinal_mach_ticks_to_nanos_v1(3, 125, 3).unwrap(), 125);
    }

    #[test]
    fn longitudinal_process_cpu_floors_fractional_nanoseconds() {
        assert_eq!(longitudinal_mach_ticks_to_nanos_v1(1, 2, 3).unwrap(), 0);
    }

    #[test]
    fn longitudinal_process_cpu_rejects_zero_denominator() {
        assert!(matches!(
            longitudinal_mach_ticks_to_nanos_v1(1, 1, 0),
            Err(LongitudinalProcessCaptureError::InvalidTimebase {
                numerator: 1,
                denominator: 0,
            })
        ));
    }

    #[test]
    fn longitudinal_process_cpu_rejects_zero_numerator() {
        assert!(matches!(
            longitudinal_mach_ticks_to_nanos_v1(1, 0, 1),
            Err(LongitudinalProcessCaptureError::InvalidTimebase {
                numerator: 0,
                denominator: 1,
            })
        ));
    }

    #[test]
    fn longitudinal_process_cpu_rejects_u64_overflow() {
        assert!(matches!(
            longitudinal_mach_ticks_to_nanos_v1(u64::MAX, u32::MAX, 1),
            Err(LongitudinalProcessCaptureError::CpuTimeOverflow)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn longitudinal_process_snapshot_matches_independent_cpu_clock() {
        fn getrusage_cpu_nanos() -> u64 {
            use std::mem::MaybeUninit;

            let mut usage = MaybeUninit::<libc::rusage>::uninit();
            assert_eq!(
                unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
                0
            );
            let usage = unsafe { usage.assume_init() };
            let timeval_nanos = |value: libc::timeval| {
                u64::try_from(value.tv_sec).unwrap() * 1_000_000_000
                    + u64::try_from(value.tv_usec).unwrap() * 1_000
            };
            timeval_nanos(usage.ru_utime) + timeval_nanos(usage.ru_stime)
        }

        let pid = std::process::id();
        let reference_before = getrusage_cpu_nanos();
        let snapshot_before = capture_longitudinal_process_snapshot_v1(pid).unwrap();
        while getrusage_cpu_nanos() - reference_before < 25_000_000 {
            std::hint::black_box((0_u64..10_000).fold(0_u64, u64::wrapping_add));
        }
        let snapshot_after = capture_longitudinal_process_snapshot_v1(pid).unwrap();
        let reference_after = getrusage_cpu_nanos();

        assert_eq!(snapshot_after.pid, pid);
        assert!(snapshot_after.resident_bytes > 0);
        assert!(matches!(
            snapshot_after.cpu_clock,
            LongitudinalProcessCpuClockV1::MachAbsoluteTime {
                numerator,
                denominator,
            } if numerator > 0 && denominator > 0
        ));
        let reference_delta = reference_after - reference_before;
        let snapshot_delta = (snapshot_after.user_cpu_nanos + snapshot_after.system_cpu_nanos)
            - (snapshot_before.user_cpu_nanos + snapshot_before.system_cpu_nanos);
        let tolerance = reference_delta / 10 + 2_000_000;
        assert!(
            snapshot_delta.abs_diff(reference_delta) <= tolerance,
            "normalized snapshot delta {snapshot_delta}ns differs from independent \
             getrusage delta {reference_delta}ns by more than {tolerance}ns"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn longitudinal_process_snapshot_rejects_unrepresentable_pid() {
        assert!(matches!(
            capture_longitudinal_process_snapshot_v1(u32::MAX),
            Err(LongitudinalProcessCaptureError::InvalidProcessId { pid: u32::MAX })
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn longitudinal_process_snapshot_rejects_unsupported_platforms() {
        assert!(matches!(
            capture_longitudinal_process_snapshot_v1(std::process::id()),
            Err(LongitudinalProcessCaptureError::UnsupportedPlatform)
        ));
    }
}
