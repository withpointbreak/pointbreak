import type {
  ChangeInspectorAccess,
  ChangeRecoveryAction,
  ChangeRecoveryStatus,
} from "./change-recovery-protocol";

export interface ChangeRecoveryPresentation {
  status: ChangeRecoveryStatus | null;
  error: string | null;
  access: ChangeInspectorAccess;
  fallbackValidated: boolean;
  pending: "fallback" | "derived" | "retry" | "cancel" | null;
}

export type ChangeRecoveryActions = Record<
  "wait" | "fallback" | "derived" | "retry" | "cancel",
  () => void
>;

export function renderChangeRecovery(
  view: ChangeRecoveryPresentation,
  callbacks: ChangeRecoveryActions,
): void {
  const root = document.querySelector<HTMLElement>("#derived-access-status");
  if (root === null) return;
  const find = <T extends HTMLElement>(selector: string) =>
    root.querySelector<T>(selector);
  const status = view.status;
  const visible =
    view.error !== null ||
    view.access === "authoritative" ||
    (status?.active === true &&
      (!status.servingCurrent ||
        status.rebuildInFlight ||
        status.rebuildPaused));
  root.classList.toggle("hidden", !visible);
  if (!visible) return;
  const summary = find<HTMLElement>("#derived-access-summary");
  const detail = find<HTMLElement>("#derived-access-detail");
  const count =
    status?.completedEvents !== undefined && status.totalEvents !== undefined
      ? ` ${status.completedEvents}/${status.totalEvents} events.`
      : "";
  if (summary !== null) {
    summary.textContent = view.error
      ? "Recovery status unavailable"
      : view.pending === "fallback"
        ? "Reading authoritative journal"
        : view.access === "authoritative"
          ? view.fallbackValidated
            ? "Authoritative fallback"
            : "Reading authoritative journal"
          : status?.rebuildPaused
            ? "Derived view rebuild paused"
            : `Derived view: ${(status?.phase ?? status?.availability ?? "unavailable").replaceAll("_", " ")}.${count}`;
  }
  if (detail !== null) detail.textContent = view.error ?? status?.detail ?? "";
  const progress = find<HTMLProgressElement>("#derived-access-progress");
  if (progress !== null) {
    const determinate =
      status?.completedEvents !== undefined && (status.totalEvents ?? 0) > 0;
    progress.classList.toggle("hidden", !determinate);
    if (determinate) {
      progress.max = status.totalEvents ?? 1;
      progress.value = Math.min(
        status.completedEvents ?? 0,
        status.totalEvents ?? 1,
      );
    }
  }
  const supported = new Set<ChangeRecoveryAction>(status?.actions ?? []);
  const buttons = [
    ["wait", "wait", callbacks.wait],
    ["fallback", "authoritative_fallback", callbacks.fallback],
    ["cancel", "cancel", callbacks.cancel],
    ["retry", "retry", callbacks.retry],
  ] as const;
  for (const [name, action, callback] of buttons) {
    const button = find<HTMLButtonElement>(`#derived-access-${name}`);
    if (button === null) continue;
    button.classList.toggle("hidden", !supported.has(action));
    button.disabled =
      view.pending !== null || status?.fallbackInFlight === true;
    button.onclick = callback;
  }
  const derived = find<HTMLButtonElement>("#derived-access-use-derived");
  if (derived !== null) {
    derived.classList.toggle("hidden", view.access !== "authoritative");
    derived.disabled = view.pending !== null;
    derived.onclick = callbacks.derived;
  }
}
