//! Baseline measurement harness for the durable event store's file backend.
//!
//! Run with `cargo bench --features bench`. It measures the three metrics a
//! future log-structured backend would be compared against: whole-log read
//! latency, single-append latency, and on-disk amplification. The synthetic
//! groups are generated in-process and need nothing external — anyone can run
//! them. An optional real-world read-all sample runs only when
//! `SHORE_BENCH_FIXTURE` points at an existing store directory; it is skipped
//! otherwise, so the harness has no baked-in paths. No alternative backend is
//! built here; this only establishes the file backend's numbers.

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput};
use shoreline::bench_support::StoreBenchHarness;

/// Synthetic store sizes. The largest is where a many-small-files layout starts
/// to slow whole-log reads.
const SIZES: &[usize] = &[100, 1_000, 10_000];

/// Whole-log read latency — the dominant access pattern, since projection,
/// inventory, and bundle all read the full event log. Measured over synthetic
/// stores and, when present, the developer-local captured fixture.
fn read_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_all");
    for &n in SIZES {
        let dir = tempfile::tempdir().expect("a temp dir");
        let harness = StoreBenchHarness::open(dir.path().join(".shore/data"));
        harness.populate(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("synthetic", n), &harness, |b, h| {
            b.iter(|| black_box(h.read_all()));
        });
    }

    match fixture_store_dir() {
        Some(store_dir) => {
            let harness = StoreBenchHarness::open(&store_dir);
            match harness.try_read_all() {
                Ok(count) => {
                    group.throughput(Throughput::Elements(count as u64));
                    group.bench_with_input(BenchmarkId::new("fixture", count), &harness, |b, h| {
                        b.iter(|| black_box(h.read_all()));
                    });
                }
                Err(error) => eprintln!(
                    "skipping SHORE_BENCH_FIXTURE read-all ({}): {error}",
                    store_dir.display()
                ),
            }
        }
        None => eprintln!(
            "skipping fixture read-all: set SHORE_BENCH_FIXTURE to a store directory \
             for a real-world sample"
        ),
    }

    group.finish();
}

/// Single-append latency. Append cost is independent of warm-store size (one
/// exclusive create), so the warm store only ensures the events directory
/// already exists; each iteration writes a fresh key so it is a genuine create,
/// never an idempotent no-op.
fn append(c: &mut Criterion) {
    let mut group = c.benchmark_group("append");
    for &n in SIZES {
        let dir = tempfile::tempdir().expect("a temp dir");
        let harness = StoreBenchHarness::open(dir.path().join(".shore/data"));
        harness.populate(n);
        group.bench_with_input(BenchmarkId::new("warm", n), &harness, |b, h| {
            b.iter(|| h.append_one());
        });
    }
    group.finish();
}

/// On-disk amplification is a one-shot measurement, not a timing — report it
/// before the timed groups so a run always surfaces it.
fn report_disk_amplification() {
    eprintln!("disk amplification — file backend, synthetic events (on-disk / logical bytes):");
    for &n in SIZES {
        let dir = tempfile::tempdir().expect("a temp dir");
        let harness = StoreBenchHarness::open(dir.path().join(".shore/data"));
        harness.populate(n);
        let usage = harness.byte_usage();
        let ratio = if usage.logical == 0 {
            0.0
        } else {
            usage.physical as f64 / usage.logical as f64
        };
        eprintln!(
            "  n={n:>6}: logical={:>10} B  on-disk={:>10} B  amplification={ratio:.2}x",
            usage.logical, usage.physical
        );
    }
}

/// An optional real-world store to sample read-all over, supplied entirely by the
/// caller through `SHORE_BENCH_FIXTURE` — no path is baked into the source, so the
/// harness stays runnable by anyone. Returns the store directory only when the env
/// var is set and its event log is present; an unset or absent fixture is skipped
/// rather than failing the run.
fn fixture_store_dir() -> Option<PathBuf> {
    let store_dir = PathBuf::from(std::env::var_os("SHORE_BENCH_FIXTURE")?);
    store_dir.join("events").is_dir().then_some(store_dir)
}

fn main() {
    report_disk_amplification();
    let mut criterion = Criterion::default().configure_from_args();
    read_all(&mut criterion);
    append(&mut criterion);
    criterion.final_summary();
}
