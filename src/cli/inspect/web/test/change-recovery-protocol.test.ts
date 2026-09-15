import { describe, expect, it } from "vitest";
import {
  decodeChangeRecoveryFailure,
  decodeChangeRecoveryStatus,
} from "../src/change-recovery-protocol";

const status = {
  schema: "pointbreak.inspect-derived-access-status",
  version: 1,
  active: true,
  availability: "rebuild_required",
  namespace: "stable",
  generationId: "generation:one",
  phase: "projection_population",
  completedEvents: 3,
  totalEvents: 9,
  completedBytes: 12,
  elapsedMilliseconds: 20,
  etaMilliseconds: 40,
  detail: "building",
  rebuildInFlight: true,
  rebuildPaused: false,
  servingCurrent: false,
  fallbackInFlight: false,
  actions: ["wait", "authoritative_fallback", "cancel"],
};

describe("Change recovery protocol", () => {
  it("decodes the complete status and filters unknown actions", () => {
    expect(
      decodeChangeRecoveryStatus({
        ...status,
        actions: [...status.actions, "future_action"],
      }),
    ).toEqual(status);
  });

  it.each([
    { ...status, version: 2 },
    { ...status, availability: "future" },
    { ...status, phase: "future" },
    { ...status, completedEvents: -1 },
    { ...status, totalEvents: Number.POSITIVE_INFINITY },
    { ...status, actions: "retry" },
    { ...status, servingCurrent: "yes" },
  ])("rejects malformed or unsupported status %#", (document) => {
    expect(decodeChangeRecoveryStatus(document)).toBeNull();
  });

  it("accepts omitted progress without inventing values", () => {
    const decoded = decodeChangeRecoveryStatus({
      ...status,
      phase: undefined,
      completedEvents: undefined,
      totalEvents: undefined,
      completedBytes: undefined,
      elapsedMilliseconds: undefined,
      etaMilliseconds: undefined,
      detail: undefined,
    });
    expect(decoded).not.toBeNull();
    expect(decoded).not.toHaveProperty("completedEvents");
    expect(decoded).not.toHaveProperty("etaMilliseconds");
  });

  it.each([
    [
      503,
      {
        schema: "pointbreak.inspect-change-projection-error",
        version: 1,
        code: "projection_invalid",
        message: "projection cannot serve this read",
        retryable: false,
      },
      "projection",
    ],
    [
      409,
      {
        schema: "pointbreak.inspect-change-page-error",
        version: 1,
        code: "stale_projection",
        message: "projection moved",
        retryable: true,
      },
      "page",
    ],
    [
      503,
      {
        schema: "pointbreak.inspect-event-history-error",
        version: 1,
        code: "moving_journal",
        message: "journal moved",
        retryable: true,
      },
      "page",
    ],
    [
      400,
      {
        schema: "pointbreak.inspect-change-selection-error",
        version: 1,
        code: "invalid_exact_selection",
        message: "selection is invalid",
      },
      "selection",
    ],
    [
      409,
      {
        schema: "pointbreak.inspect-change-authority-error",
        version: 1,
        code: "authority_conflicted",
        message: "authority is conflicted",
      },
      "authority",
    ],
    [
      409,
      {
        schema: "pointbreak.store-migration-required",
        version: 1,
        state: "migration_required",
      },
      "capability",
    ],
    [
      426,
      {
        schema: "pointbreak.reader-upgrade-required",
        version: 1,
        code: "reader_upgrade_required",
        message: "upgrade required",
      },
      "capability",
    ],
  ] as const)("preserves typed failure %#", (httpStatus, document, kind) => {
    expect(decodeChangeRecoveryFailure(document, httpStatus)).toMatchObject({
      kind,
      schema: document.schema,
      status: httpStatus,
    });
  });

  it("fails closed for unknown codes and malformed recognized envelopes", () => {
    expect(
      decodeChangeRecoveryFailure(
        {
          schema: "pointbreak.inspect-change-projection-error",
          version: 1,
          code: "future_projection_state",
          message: "future",
          retryable: false,
        },
        503,
      ),
    ).toBeNull();
    expect(
      decodeChangeRecoveryFailure(
        {
          schema: "pointbreak.inspect-change-projection-error",
          version: 1,
          code: "projection_invalid",
          message: 7,
          retryable: false,
        },
        503,
      ),
    ).toBeNull();
    expect(
      decodeChangeRecoveryFailure(
        {
          schema: "pointbreak.inspect-change-page-error",
          version: 1,
          code: "stale_projection",
          message: "projection moved",
          retryable: true,
        },
        503,
      ),
    ).toBeNull();
    expect(
      decodeChangeRecoveryFailure(
        {
          schema: "pointbreak.store-migration-required",
          version: 1,
          state: "migration_in_progress",
        },
        409,
      ),
    ).toBeNull();
  });
});
