import { expect, it } from "vitest";
import pkg from "../package.json";

it("never activates on startup (lazy activation only)", () => {
  expect(pkg.activationEvents ?? []).not.toContain("onStartupFinished");
  expect(pkg.activationEvents ?? []).not.toContain("*");
});

it("carries the untouchable identity and license", () => {
  expect(pkg.publisher).toBe("pointbreak");
  expect(pkg.name).toBe("pointbreak");
  expect(pkg.license).toBe("Apache-2.0");
});
