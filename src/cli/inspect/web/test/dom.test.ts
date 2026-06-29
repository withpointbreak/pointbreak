import { afterEach, describe, expect, it } from "vitest";
import { $ } from "../src/dom";
import { mountInspectorDom, resetDom } from "./support/dom";

afterEach(() => {
  resetDom();
});

describe("$", () => {
  it("resolves a selector to its element", () => {
    mountInspectorDom();
    const master = $("#master");
    expect(master).not.toBeNull();
    expect(master?.id).toBe("master");
  });

  it("returns null when nothing matches (the app.js querySelector contract)", () => {
    mountInspectorDom();
    expect($("#no-such-element")).toBeNull();
  });

  it("narrows to a typed element via the generic parameter, no cast", () => {
    mountInspectorDom();
    const input = $<HTMLInputElement>("#filter-text");
    expect(input).not.toBeNull();
    expect(input?.tagName).toBe("INPUT");
    // The generic hands callers the concrete element type (`.value` here)
    // without a cast or a non-null assertion.
    expect(input?.value).toBe("");
  });
});
