# Local native ownership ABI

The four private writer profiles permit the fixed no-argument synchronous and
foreign_keys readbacks used by configuration, in addition to their existing
FULL/ON setters. Reader and unrelated PRAGMA permissions remain restricted.
The Rust WAL-mode setter checks one result row, terminal completion and explicit
statement finalization, retaining the first error before further configuration.
These configuration changes have compiled; broad runtime acceptance remains pending.

The private native ownership interface exposes reader/writer open, visit, barrier
and explicit finish with the implemented ownership capability set. Public store
routing remains disabled; selected private macOS checks do not qualify the full
contract or Windows. Qualification observer remains a conditional declaration.

The root manifest pins libsqlite3-sys exactly to 0.38.1 and patches crates.io to
this directory for both direct and rusqlite dependency edges. Its path lock entry
omits the registry source/checksum. A directory manifest must bind patched bytes;
the original crate checksum alone cannot identify them.

## Build and context

Compiler admission tracks absent CC/flag inputs and rejects compiler arguments,
known wrappers, MSVC CL/_CL_ overrides and target-qualified flag overrides,
external native flags, inherited Rust flags and indirect preprocessor
configuration. The closed macro set and resulting C guards require threadsafety,
authorization/WAL, API armor, compatible memory temporaries and native locking.
Only macOS aarch64/x86_64 and Windows MSVC aarch64/x86_64 are selected. These checks
are authored source, not evidence that the compiler accepts this configuration.

The internal context flows from sqlite3 through Btree/Pager/Wal to per-file native
structures before I/O. Ordinary public opens pass NULL. The public sqlite3_file
prefix is unchanged. Shared inode/SHM nodes and deferred descriptors never retain
a callback context. Their upstream structures, locks and refcounts are preserved.

## Dormant ownership operations

The private open validates sizes/version, fixed profile, reserved fields, fixed
native VFS and absolute path. It owns a stable heap context independently of the
SQLite connection; non-null failed/sick connections remain owned until explicit
finish. Private automatic/compiled-in extensions and automatic checkpoint setup
are skipped. Temp storage and initial mmap limits are connection-local. The
internal authorizer cannot be replaced through sqlite3_set_authorizer. Writer
PERSIST_WAL is set and queried before returning a successful private open;
NO_CKPT_ON_CLOSE is present before Btree work. These paths require the private owned-process entry and exact native qualification.

Visit enumerates actual borrowed main/WAL/journal and SHM mapping/locking handles
and compares the cached admitted role inventory. Synchronous callback events and
finish records carry a generation. Reader/writer profile and readonly-node checks
remain distinct. A writer barrier inventories before and after syncing the actual
main/WAL descriptors with fsync plus F_FULLFSYNC, or FlushFileBuffers. It opens no
replacement handle. Checkpoint ordering, caller gates and hook integration remain
unfinished consumers.

Finish finalizes statements, rolls back a live transaction and calls non-zombie
close while the context lives. It consumes the slot only after destruction and
complete detached/shared/closed role dispositions. Native close observations run
before teardown; first errors do not suppress retirement. Known main/native close
and unmap uncertainty invokes the private terminal callback and immediate worker
exit if it returns. Unix association failure does so before a potentially unsafe
close, including allocation failure without proven cross-role disjointness. This
policy is limited to the dedicated private worker; it is not a shared-host policy.

## Remaining native obligations

Native admission and SHM lifecycle hooks are now authored as described below.
Dormant journal routing and reader fences are now present as described below.
The complete affected lifecycle still requires coupled source review, matching
physical identity consumers and platform qualification. Native source hooks have
no compile or runtime acceptance. The fixed authorizer is an additional guard,
not an exact-SQL parser or proof that every engine mutation path is fenced. Never enable a capability or
remove a native refusal until its whole affected lifecycle is implemented and
reviewed together. Product Rust ownership wrappers, fixed SQL routing and
qualification instrumentation remain separate unfinished consumers.

The hand-maintained Rust declarations at the end of the bundled binding file
must match both C header copies, including generations and role masks. Layout,
linkage, failed-open/close and native behavior need later authorized checks.
Qualification declarations are behind pointbreak-qualification and are not
enabled by the root manifest.

The Pointbreak source maintainer owns this delta. For every upstream, compiler,
feature or native-method change: refresh the complete baseline inventory, audit
all affected open/map/lock/close and error paths, review the authored delta and
C/Rust ABI layout, then run the affected authorized macOS and Windows controls.
An upstream version label cannot carry old evidence. Count every authored
added/replacement code, binding and build line, including blanks; retain unchanged
imported baseline separately. Deletions do not refund authored work. Never
regenerate bindings over the private ABI without reviewing the exact result.

## Native admission and shared memory source

Dormant Unix main/WAL/file admission now checks the actual fd against SQLite's
existing main/SHM registry under its mutex before invoking borrowed C3 admission.
The callback remains responsible for exact path/native identity, N/Q and cross-role
disjointness. An unverified newly owned regular handle cannot be closed on an
admission failure. The close wrapper permits only admitted retiring roles, known
upstream deferred descriptors with no remaining locks, or verified directory
handles. No callback is retained by the registry or pending descriptor list.

Private Unix opens skip descriptor reuse, permission normalization, ownership
adjustment, writable fallback and auxiliary low-fd opens. Low fds are rejected
only after association/admission via normal close/defer. The zero-length MSDOS
workaround refuses before its write. Private Windows opens add reparse-point
handling and skip retry/writable-to-readonly fallback; callback admission occurs
on the actual owned native handle. Closed native flag/type checks and first-error
propagation are present before dispatch. Reader WAL requests existing readonly
access. Neither native pre-I/O refusal has been removed.

Private SHM opens admit mapping and locking roles before use/linking. Unix uses
existing readonly handles for readers and0600 creation for writers, without
normalization. Windows has a separate private attach path, OPEN_EXISTING for
readers and no fallback. Existing nodes must match readonly/writable permissions;
callbacks validate their actual handles. Reopening a retired SHM binding refuses.
The native DMS/CANTINIT algorithm remains responsible for genuine heap fallback.
Before mapping, actual handles are visited; readonly insufficient-size requests
refuse before extension/truncation or larger mapping creation. A failed C3 check
cannot expose a map as a successful readonly result.

Retirement callbacks precede SHM detach/close, including partial admission cleanup.
Unix refcounts and pending-fd ownership remain upstream. Windows private purge is
restricted to the closing connection's node, with no unrelated purge attribution.
Native shared/closed dispositions distinguish retained sibling resources and the
separate Windows lock handle. Private unmap/unlock/close uncertainty terminates
before normal unwinding, and private close never deletes SHM. No platform syscall
or new source path has been executed. Product/helper callbacks, profile integration,
whole-engine mutation/error coverage and independent source/platform checks remain
required before any capability can be enabled.

## Dormant journal and reader fences

Private journal presence uses descriptor-free native metadata, including empty
files; unknown access errors refuse. This is presence, not physical identity
proof. Writer hot-journal probes/recovery and new main journals route through the
connection-bound native admission function. The actual main journal opens
immediately instead of spilling an unowned memory wrapper. Subjournals remain
memory-only. Journal deletion requires completed native retirement of the bound
journal role; a missing ownership prerequisite refuses. Private super-journal
namespaces are unsupported, with refusal before external access or cleanup.

Readers refuse any rollback-journal presence, missing WAL, short/unsupported main
headers and mode mismatch. Native write/truncate/sync, persistent file controls,
Pager/WAL writer and recovery paths are fenced. WAL_SHM_RDONLY is sticky. Genuine
CANTINIT still selects SQLite's heap-index algorithm with its native locks and
checks; WAL_RETRY becomes a refused snapshot before another attempt. Heap-index
teardown also detaches the actual SHM attachment without deletion. Reader close
uses unlock/cache cleanup and cannot synchronize or replay a journal.

Private main/WAL fetch returns no mapped page. Temporary Btrees may use the
existing memory Pager, with no native file owner; persistent temp and sorter
spill entries refuse. A reader open begins its deferred transaction before caller
schema/stamp/H0 queries. Statements outside that transaction refuse; prepare and
step preserve failures, including denied configuration and callback reentry.
Public descriptor, blob, backup, extension/configuration and automatic WAL hook
surfaces refuse. Built-in allowed pragmas skip the public VFS override path.
Writer rollback permissions remain distinct from reader mutation refusal.

Private capabilities are enabled for the owned entry; native acceptance remains
limited to the exact cases and source actually exercised. Complete native error,
cleanup and concurrent behavior, C/Rust ownership consumers, exact caller SQL and
qualification profiles still require integrated review and platform controls.

## Reader transaction bootstrap

The private reader establishes a deferred transaction before returning its handle.
SQLite marks BEGIN itself as a reader statement. Only the exact engine-owned
prepared BEGIN is admitted while autocommit is enabled; its identity is revoked
before finalization on every path. Success requires terminal completion and
autocommit off. Caller data reads outside the transaction and callback reentry
remain refused; failed open retains native ownership through explicit finish.
