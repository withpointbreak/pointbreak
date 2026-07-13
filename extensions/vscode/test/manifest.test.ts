import { expect, it } from "vitest";
import pkg from "../package.json";

it("activates only for explicit Pointbreak surfaces", () => {
  expect(pkg.activationEvents ?? []).not.toContain("onStartupFinished");
  expect(pkg.activationEvents ?? []).not.toContain("*");
});

it("carries the untouchable identity and license", () => {
  expect(pkg.publisher).toBe("pointbreak");
  expect(pkg.name).toBe("pointbreak");
  expect(pkg.license).toBe("Apache-2.0");
});

it("contributes the Review view and its commands", () => {
  expect(pkg.activationEvents).toEqual([
    "onView:pointbreak.attention",
    "onCommand:pointbreak.refreshAttention",
    "onCommand:pointbreak.capture",
    "onCommand:pointbreak.openInReview",
    "onCommand:pointbreak.stopInspect",
  ]);
  expect(pkg.contributes.views.pointbreak).toContainEqual({
    id: "pointbreak.attention",
    name: "Review",
  });
  expect(pkg.contributes.commands.map(({ command }) => command)).toEqual([
    "pointbreak.refreshAttention",
    "pointbreak.capture",
    "pointbreak.openInReview",
    "pointbreak.stopInspect",
  ]);
  expect(
    pkg.contributes.configuration.properties["pointbreak.reviewUrl"],
  ).toMatchObject({
    default: "",
    scope: "resource",
    description:
      "Optional URL for an externally managed Pointbreak Review server for this folder.",
  });
  expect(
    pkg.contributes.configuration.properties["pointbreak.reviewUrl"]
      .description,
  ).not.toMatch(/restore|remember|port/i);
});
