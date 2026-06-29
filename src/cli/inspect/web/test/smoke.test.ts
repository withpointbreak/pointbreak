import { describe, expect, it } from "vitest";

describe("harness", () => {
  it("runs under happy-dom", () => {
    const el = document.createElement("div");
    el.innerHTML = '<span role="note">ok</span>';
    expect(el.querySelector("[role=note]")?.textContent).toBe("ok");
  });
});
