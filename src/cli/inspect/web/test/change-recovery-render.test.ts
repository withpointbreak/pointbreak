import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeRecoveryStatus } from "../src/change-recovery-protocol";
import { renderChangeRecovery } from "../src/change-recovery-render";
import { mountInspectorDom, resetDom } from "./support/dom";

const status: ChangeRecoveryStatus = {
  schema: "pointbreak.inspect-derived-access-status",
  version: 1,
  active: true,
  availability: "rebuild_required",
  namespace: "stable",
  detail: "<b>server text</b>",
  rebuildInFlight: false,
  rebuildPaused: false,
  servingCurrent: false,
  fallbackInFlight: false,
  actions: ["authoritative_fallback", "retry"],
};
const actions = {
  wait: vi.fn(),
  fallback: vi.fn(),
  derived: vi.fn(),
  retry: vi.fn(),
  cancel: vi.fn(),
};

beforeEach(() => {
  resetDom();
  mountInspectorDom();
  for (const action of Object.values(actions)) action.mockClear();
});

describe("Change recovery presentation", () => {
  it("shows an unknown outcome without claiming cancellation completed", () => {
    renderChangeRecovery(
      {
        status: {
          ...status,
          actions: ["retry"],
          child: {
            epoch: 2,
            operation: 7,
            requestId: "a".repeat(32),
            lastCompletedOperation: null,
            lastCompletedRequestId: null,
            lastCompletedKind: null,
            lastCompletedOutcome: null,
            state: "unknown",
            detail: "Response lost; operation may still be running.",
            cooperativeCancellation: true,
            observationOnly: true,
            observationPending: false,
            observedAtUnixMilliseconds: null,
            observationAgeMilliseconds: null,
          },
        },
        error: null,
        access: "derived",
        fallbackValidated: false,
        pending: null,
        statusPending: false,
      },
      actions,
    );
    expect(document.querySelector("#derived-access-summary")?.textContent).toBe(
      "Data operation outcome unknown",
    );
    expect(
      document.querySelector("#derived-access-detail")?.textContent,
    ).toContain("may still be running");
    expect(
      document.querySelector("#derived-access-cancel")?.classList,
    ).toContain("hidden");
  });
  it("shows only advertised actions and treats server detail as text", () => {
    renderChangeRecovery(
      {
        status,
        error: null,
        access: "derived",
        fallbackValidated: false,
        pending: null,
        statusPending: false,
      },
      actions,
    );

    expect(
      document.querySelector("#derived-access-status")?.classList,
    ).not.toContain("hidden");
    expect(document.querySelector("#derived-access-detail")?.textContent).toBe(
      "<b>server text</b>",
    );
    expect(document.querySelector("#derived-access-detail b")).toBeNull();
    expect(
      document.querySelector("#derived-access-fallback")?.classList,
    ).not.toContain("hidden");
    expect(
      document.querySelector("#derived-access-retry")?.classList,
    ).not.toContain("hidden");
    expect(
      document.querySelector("#derived-access-cancel")?.classList,
    ).toContain("hidden");
  });

  it("labels pending and validated fallback without inventing progress", () => {
    renderChangeRecovery(
      {
        status: { ...status, totalEvents: 0, completedEvents: 0 },
        error: null,
        access: "authoritative",
        fallbackValidated: false,
        pending: "fallback",
        statusPending: false,
      },
      actions,
    );
    expect(document.querySelector("#derived-access-summary")?.textContent).toBe(
      "Reading authoritative journal",
    );
    expect(
      document.querySelector("#derived-access-progress")?.classList,
    ).toContain("hidden");
    expect(
      document.querySelector<HTMLButtonElement>("#derived-access-retry")
        ?.disabled,
    ).toBe(true);

    renderChangeRecovery(
      {
        status,
        error: null,
        access: "authoritative",
        fallbackValidated: true,
        pending: null,
        statusPending: false,
      },
      actions,
    );
    expect(document.querySelector("#derived-access-summary")?.textContent).toBe(
      "Authoritative fallback",
    );
    document
      .querySelector<HTMLButtonElement>("#derived-access-use-derived")
      ?.click();
    expect(actions.derived).toHaveBeenCalledOnce();
  });

  it("offers only a read-only recheck when status observation fails", () => {
    renderChangeRecovery(
      {
        status: null,
        error: "server unavailable",
        access: "derived",
        fallbackValidated: false,
        pending: null,
        statusPending: false,
      },
      actions,
    );
    expect(document.querySelector("#derived-access-summary")?.textContent).toBe(
      "Recovery status unavailable",
    );
    const recheck = document.querySelector<HTMLButtonElement>(
      "#derived-access-wait",
    );
    expect(recheck?.classList).not.toContain("hidden");
    expect(recheck?.textContent).toBe("Check status again");
    expect(recheck?.disabled).toBe(false);
    recheck?.click();
    expect(actions.wait).toHaveBeenCalledOnce();
    for (const id of ["fallback", "cancel", "retry"])
      expect(
        document.querySelector(`#derived-access-${id}`)?.classList,
      ).toContain("hidden");
    expect(actions.fallback).not.toHaveBeenCalled();
    expect(actions.cancel).not.toHaveBeenCalled();
    expect(actions.retry).not.toHaveBeenCalled();

    renderChangeRecovery(
      {
        status: { ...status, actions: ["wait"] },
        error: null,
        access: "derived",
        fallbackValidated: false,
        pending: null,
        statusPending: false,
      },
      actions,
    );
    expect(recheck?.textContent).toBe("wait");
    expect(recheck?.classList).not.toContain("hidden");
  });
});
