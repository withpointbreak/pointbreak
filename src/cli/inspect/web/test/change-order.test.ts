import { describe, expect, it } from "vitest";
import {
  formatChangeInspectorRoute,
  parseChangeInspectorRoute,
  queryForExactNavigation,
  queryForLens,
} from "../src/change-inspector-router";
import type { ChangePageOrder, ChangeSummary } from "../src/change-protocol";
import {
  buildChangePageUrl,
  compareEventInstants,
  DEFAULT_CHANGE_PAGE_ORDER,
  decodeChangePage,
  isChangePageOrderAdmitted,
  isStrictlyOrderedChangePage,
} from "../src/change-protocol";
import parity from "./fixtures/change-order-parity.json";

type SummaryFixture = Partial<ChangeSummary> & { changeId: string };

function summary(fixture: SummaryFixture, stamp: string): ChangeSummary {
  return {
    declarationState: "authoritative",
    titleAssertions: [],
    memberCount: 0,
    topology: "initial",
    lifecycle: "in_progress",
    attentionSummary: "in_progress",
    availabilitySummary: "available",
    currentRevisionRefs: [],
    projectionStamp: stamp,
    ...fixture,
  };
}

function pageFixture(
  rows: SummaryFixture[],
  options: { order?: ChangePageOrder; lens?: "changes" | "attention" } = {},
): Record<string, unknown> {
  const stamp = "sha256:generation";
  const lens = options.lens ?? "changes";
  return {
    schema:
      lens === "changes"
        ? "pointbreak.inspect-changes-page"
        : "pointbreak.inspect-attention",
    version: lens === "changes" ? 1 : 2,
    projectionStamp: stamp,
    ...(options.order === undefined ? {} : { order: options.order }),
    next: null,
    changes: rows.map((row) => summary(row, stamp)),
  };
}

function ids(page: { changes: ChangeSummary[] }): string[] {
  return page.changes.map((change) => change.changeId);
}

describe("Change page order (client admission)", () => {
  it("defaults the Change page order per lens", () => {
    expect(buildChangePageUrl("changes")).toContain("order=activity_desc");
    expect(buildChangePageUrl("attention")).toContain("order=attention_wait");
    expect(DEFAULT_CHANGE_PAGE_ORDER).toEqual({
      changes: "activity_desc",
      attention: "attention_wait",
    });
  });

  it("accepts change_id_asc explicitly on both lenses", () => {
    expect(buildChangePageUrl("changes", { order: "change_id_asc" })).toContain(
      "order=change_id_asc",
    );
    expect(
      buildChangePageUrl("attention", { order: "change_id_asc" }),
    ).toContain("order=change_id_asc");
  });

  it("rejects any unknown order value and attention_wait on the Changes lens", () => {
    expect(() =>
      buildChangePageUrl("changes", {
        order: "activity_asc" as ChangePageOrder,
      }),
    ).toThrow(/order/);
    expect(() =>
      buildChangePageUrl("changes", { order: "attention_wait" }),
    ).toThrow(/order/);
    expect(
      buildChangePageUrl("attention", { order: "attention_wait" }),
    ).toContain("order=attention_wait");
    expect(isChangePageOrderAdmitted("changes", "attention_wait")).toBe(false);
    expect(isChangePageOrderAdmitted("attention", "attention_wait")).toBe(true);
  });

  it("parses optional activityAt and attentionWaitAt on a Change summary", () => {
    const page = decodeChangePage(
      pageFixture([
        {
          changeId: "change:sha256:0a1f",
          activityAt: "2026-09-12T19:20:00.000Z",
          attentionWaitAt: {
            tierRank: 0,
            oldestObservedAt: "2026-09-01T00:00:00.000Z",
          },
        },
      ]),
      { lens: "changes", bounded: false },
    );

    expect(page.changes[0]?.activityAt).toBe("2026-09-12T19:20:00.000Z");
    expect(page.changes[0]?.attentionWaitAt).toEqual({
      tierRank: 0,
      oldestObservedAt: "2026-09-01T00:00:00.000Z",
    });
    expect(page.order).toBe("activity_desc");
  });

  it("accepts a Change summary with no activityAt", () => {
    const page = decodeChangePage(
      pageFixture([{ changeId: "change:sha256:0a1f" }]),
      {
        lens: "changes",
        bounded: false,
      },
    );

    expect(page.changes[0]?.activityAt).toBeUndefined();
    expect(page.changes[0]?.attentionWaitAt).toBeUndefined();
  });

  it("rejects a malformed activityAt or attentionWaitAt", () => {
    for (const row of [
      { changeId: "change:sha256:0a1f", activityAt: 7 },
      { changeId: "change:sha256:0a1f", activityAt: "" },
      {
        changeId: "change:sha256:0a1f",
        attentionWaitAt: { tierRank: "0", oldestObservedAt: "x" },
      },
      { changeId: "change:sha256:0a1f", attentionWaitAt: { tierRank: 0 } },
    ]) {
      expect(() =>
        decodeChangePage(pageFixture([row as SummaryFixture]), {
          lens: "changes",
          bounded: false,
        }),
      ).toThrow(/invalid changes Change page DTO/);
    }
  });

  it("accepts an activity_desc page instead of throwing on it", () => {
    // Guard 3 of 3: the DTO validator used to require ascending ids.
    const page = decodeChangePage(
      pageFixture([
        {
          changeId: "change:sha256:3b77",
          activityAt: "2026-09-12T19:20:00.000Z",
        },
        {
          changeId: "change:sha256:0a1f",
          activityAt: "2026-09-12T18:04:00.000Z",
        },
      ]),
      { lens: "changes", bounded: false },
    );

    expect(ids(page)).toEqual(["change:sha256:3b77", "change:sha256:0a1f"]);
  });

  it("still rejects a page with duplicate Change ids", () => {
    expect(() =>
      decodeChangePage(
        pageFixture([
          {
            changeId: "change:sha256:0a1f",
            activityAt: "2026-09-12T19:20:00.000Z",
          },
          {
            changeId: "change:sha256:0a1f",
            activityAt: "2026-09-12T18:04:00.000Z",
          },
        ]),
        { lens: "changes", bounded: false },
      ),
    ).toThrow(/invalid changes Change page DTO/);
  });

  it("still rejects a page that is mis-ordered for the order it declares", () => {
    expect(() =>
      decodeChangePage(
        pageFixture(
          [
            { changeId: "change:sha256:3b77" },
            { changeId: "change:sha256:0a1f" },
          ],
          { order: "change_id_asc" },
        ),
        { lens: "changes", bounded: false },
      ),
    ).toThrow(/invalid changes Change page DTO/);
    expect(() =>
      decodeChangePage(
        pageFixture([
          {
            changeId: "change:sha256:0a1f",
            activityAt: "2026-09-12T18:04:00.000Z",
          },
          {
            changeId: "change:sha256:3b77",
            activityAt: "2026-09-12T19:20:00.000Z",
          },
        ]),
        { lens: "changes", bounded: false },
      ),
    ).toThrow(/invalid changes Change page DTO/);
  });

  it("rejects a page declaring an order its lens does not admit", () => {
    expect(() =>
      decodeChangePage(
        pageFixture([{ changeId: "change:sha256:0a1f" }], {
          order: "attention_wait",
        }),
        { lens: "changes", bounded: false },
      ),
    ).toThrow(/invalid changes Change page DTO/);
    expect(
      decodeChangePage(
        pageFixture([{ changeId: "change:sha256:0a1f" }], {
          order: "attention_wait",
          lens: "attention",
        }),
        { lens: "attention", bounded: false },
      ).order,
    ).toBe("attention_wait");
  });

  it("renders Changes in received order without sorting locally", () => {
    const received = ["change:sha256:3b77", "change:sha256:0a1f"];

    const page = decodeChangePage(
      pageFixture([
        { changeId: received[0] ?? "", activityAt: "2026-09-12T19:20:00.000Z" },
        { changeId: received[1] ?? "", activityAt: "2026-09-12T18:04:00.000Z" },
      ]),
      { lens: "changes", bounded: false },
    );

    expect(ids(page)).toEqual(received);
  });

  it("agrees with the Rust comparator on every shared parity vector", () => {
    for (const vector of parity.instantComparisons) {
      const expected =
        vector.expected === "lt" ? -1 : vector.expected === "gt" ? 1 : 0;
      expect(
        compareEventInstants(vector.left, vector.right),
        `${vector.left} vs ${vector.right}`,
      ).toBe(expected);
    }
    for (const page of parity.activityDescPages) {
      expect(
        isStrictlyOrderedChangePage(
          page.order as ChangePageOrder,
          page.changes as Array<Partial<ChangeSummary> & { changeId: string }>,
        ),
        page.note,
      ).toBe(page.monotonic);
    }
  });
});

describe("Change page order (hash router)", () => {
  it("round-trips each lens and order through the hash router", () => {
    for (const [lens, order] of [
      ["changes", "activity_desc"],
      ["changes", "change_id_asc"],
      ["attention", "attention_wait"],
      ["attention", "activity_desc"],
      ["attention", "change_id_asc"],
    ] as const) {
      const hash = `#/${lens}?order=${order}`;
      const route = parseChangeInspectorRoute(hash);
      expect(route.kind).toBe("lens");
      if (route.kind !== "lens") throw new Error("expected a lens route");
      expect(route.lens).toBe(lens);
      expect(route.query.order).toBe(order);
      expect(formatChangeInspectorRoute(route)).toBe(hash);
    }
  });

  it("rejects attention_wait on the changes lens and on exact routes", () => {
    expect(
      parseChangeInspectorRoute("#/changes?order=attention_wait").kind,
    ).toBe("invalid");
    expect(
      parseChangeInspectorRoute(
        "#/changes/change%3Asha256%3Aone?order=attention_wait",
      ).kind,
    ).toBe("invalid");
    expect(parseChangeInspectorRoute("#/changes?order=activity").kind).toBe(
      "invalid",
    );
  });

  it("drops an order the target lens does not admit when switching lens", () => {
    expect(
      queryForLens("changes", { order: "attention_wait", limit: 20 }),
    ).toEqual({ limit: 20 });
    expect(
      queryForLens("attention", { order: "change_id_asc", limit: 20 }),
    ).toEqual({ order: "change_id_asc", limit: 20 });
    const attention = parseChangeInspectorRoute(
      "#/attention?after=page&limit=20&order=attention_wait",
    );
    if (attention.kind === "invalid") throw new Error(attention.message);
    expect(queryForExactNavigation(attention)).toEqual({ limit: 20 });
  });
});
