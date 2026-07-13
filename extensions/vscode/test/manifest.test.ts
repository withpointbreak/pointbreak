import { expect, it } from "vitest";
import pkg from "../package.json";

it("restores managed Review servers after startup without eager activation", () => {
  expect(pkg.activationEvents ?? []).toContain("onStartupFinished");
  expect(pkg.activationEvents ?? []).not.toContain("*");
});

it("carries the untouchable identity and license", () => {
  expect(pkg.publisher).toBe("pointbreak");
  expect(pkg.name).toBe("pointbreak");
  expect(pkg.license).toBe("Apache-2.0");
});

it("contributes the Review view and its commands", () => {
  expect(pkg.activationEvents).toEqual([
    "onStartupFinished",
    "onView:pointbreak.attention",
    "onCommand:pointbreak.refreshAttention",
    "onCommand:pointbreak.capture",
    "onCommand:pointbreak.openInReview",
  ]);
  expect(pkg.contributes.views.pointbreak).toContainEqual({
    id: "pointbreak.attention",
    name: "Review",
  });
  expect(pkg.contributes.commands.map(({ command }) => command)).toEqual([
    "pointbreak.refreshAttention",
    "pointbreak.capture",
    "pointbreak.openInReview",
  ]);
  expect(
    pkg.contributes.configuration.properties["pointbreak.reviewUrl"],
  ).toMatchObject({
    default: "",
    scope: "resource",
  });
});
