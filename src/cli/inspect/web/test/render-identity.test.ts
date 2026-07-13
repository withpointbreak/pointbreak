import { afterEach, beforeEach, expect, it } from "vitest";
import type { IdentityDoc } from "../src/store";
import { mountInspectorDom, resetDom } from "./support/dom";
import { installFetchMock, uninstallFetchMock } from "./support/fetch";

// `renderIdentity` (inside the single `render()` subscriber) fills the static store
// chip + detail popover (issue #391): the chip shows the repository name only, and
// the popover holds the identity rows (renderIdentity), the store counts + hash
// (renderStats, static spans), and the trust footnote (static). It also sets the
// browser tab `<title>` and hides the chip until identity loads. Module singletons
// (store, render), so reset and re-import before each test.
type Store = typeof import("../src/store");
type Render = typeof import("../src/render");
let store: Store;
let render: Render;

const CLONE: IdentityDoc = {
  storeIdentity: "store:sha256:fixture",
  contextIdentity: "context:sha256:fixture",
  repository: "pointbreak",
  placement: { tier: "clone", label: "clone store" },
};

beforeEach(async () => {
  const vitest = await import("vitest");
  vitest.vi.resetModules();
  store = await import("../src/store");
  render = await import("../src/render");
  mountInspectorDom();
  installFetchMock();
});

afterEach(() => {
  uninstallFetchMock();
  resetDom();
  document.title = "Pointbreak Review";
});

it("shows the repository in the chip, reveals it, and sets the tab title", () => {
  store.commit({ identity: CLONE });
  render.render();
  const root = document.querySelector("#store-identity");
  expect(root?.classList.contains("hidden")).toBe(false);
  expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
    "pointbreak",
  );
  expect(document.title).toBe("pointbreak · Pointbreak Review");
});

it("puts repository and placement in the detail rows", () => {
  store.commit({ identity: CLONE });
  render.render();
  const rows = document.querySelector("#store-identity-rows");
  expect(rows?.textContent).toContain("repository");
  expect(rows?.textContent).toContain("pointbreak");
  expect(rows?.textContent).toContain("store");
  expect(rows?.textContent).toContain("clone store");
});

it("omits family and worktree rows when absent", () => {
  store.commit({ identity: CLONE });
  render.render();
  const rows = document.querySelector("#store-identity-rows");
  expect(rows?.textContent).not.toContain("family");
  expect(rows?.textContent).not.toContain("worktree");
});

it("shows the family row under the user-level tier", () => {
  store.commit({
    identity: {
      ...CLONE,
      repository: "pointbreak",
      placement: { tier: "family", label: "family store" },
      family: { id: "acme-web" },
    },
  });
  render.render();
  const rows = document.querySelector("#store-identity-rows");
  expect(rows?.textContent).toContain("family");
  expect(rows?.textContent).toContain("acme-web");
});

it("shows the worktree row when present", () => {
  store.commit({
    identity: {
      ...CLONE,
      repository: "pointbreak",
      worktree: "feat-foo",
      placement: { tier: "clone", label: "clone store" },
    },
  });
  render.render();
  const rows = document.querySelector("#store-identity-rows");
  expect(rows?.textContent).toContain("worktree");
  expect(rows?.textContent).toContain("feat-foo");
});

it("exposes the full identity as the chip's accessible label", () => {
  store.commit({
    identity: {
      ...CLONE,
      repository: "pointbreak",
      placement: { tier: "family", label: "family store" },
      family: { id: "acme-web" },
    },
  });
  render.render();
  const label =
    document.querySelector("#store-chip")?.getAttribute("aria-label") ?? "";
  expect(label).toContain("pointbreak");
  expect(label).toContain("family store");
  expect(label).toContain("acme-web");
});

it("keeps the store counts and the trust footnote in the popover", () => {
  store.commit({ identity: CLONE });
  render.render();
  // The stat spans (renderStats-owned) and the trust note are static popover markup.
  expect(
    document.querySelector(".store-identity-stats #stat-events"),
  ).not.toBeNull();
  const note = document.querySelector(".store-identity-note");
  expect(note?.textContent).toContain("never gates writes");
  expect(note?.textContent).toContain("reader-relative");
});

it("hides the chip and resets the title when identity is null", () => {
  store.commit({ identity: null });
  render.render();
  expect(document.title).toBe("Pointbreak Review");
  expect(
    document.querySelector("#store-identity")?.classList.contains("hidden"),
  ).toBe(true);
});
