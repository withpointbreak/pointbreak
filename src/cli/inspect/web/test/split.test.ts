import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HistoryDoc, RevisionsDoc } from "../src/store";
import historyJson from "./fixtures/history.json";
import revisionsJson from "./fixtures/revisions.json";
import { mountInspectorDom, resetDom } from "./support/dom";
import { installFetchMock, uninstallFetchMock } from "./support/fetch";

// The divider between the split panes: static window-splitter markup plus the
// `split.ts` controller — the pointer-capture drag, the double-click reset, and
// the ArrowLeft/Right + Enter keyboard contract. Every width write goes through
// prefs.applySplit (the single `--split-master` writer), so drags persist and
// reloads restore. The involved modules are singletons — reset and re-import
// before each test.
type Store = typeof import("../src/store");
type Keyboard = typeof import("../src/keyboard");
type Split = typeof import("../src/split");
let store: Store;
let keyboard: Keyboard;
let split: Split;

const SPLIT_KEY = "shore-inspect-split";
const REV =
  "rev:sha256:9a7626ca7cb2801721ed992402184460210477aadfd4f7228628b65ff11a6efd";

function divider(): HTMLElement {
  const el = document.querySelector<HTMLElement>(".divider");
  if (!el) throw new Error(".divider not mounted");
  return el;
}

beforeEach(async () => {
  vi.resetModules();
  store = await import("../src/store");
  keyboard = await import("../src/keyboard");
  split = await import("../src/split");
  mountInspectorDom();
  installFetchMock();
  localStorage.clear();
  history.replaceState(null, "", "/");
  store.commit({
    history: historyJson as unknown as HistoryDoc,
    revisions: revisionsJson as unknown as RevisionsDoc,
  });
});

afterEach(() => {
  uninstallFetchMock();
  resetDom();
  localStorage.clear();
});

describe("the divider markup (the window-splitter contract)", () => {
  it("mounts a keyboard-reachable separator between the panes", () => {
    const el = document.querySelector('.split .divider[role="separator"]');
    expect(el).not.toBeNull();
    expect(el?.getAttribute("aria-orientation")).toBe("vertical");
    expect(el?.getAttribute("tabindex")).toBe("0");
    expect(el?.getAttribute("aria-valuemin")).toBe("25");
    expect(el?.getAttribute("aria-valuemax")).toBe("75");
  });
});

describe("the divider controller (drag / reset / keys)", () => {
  it("dragging updates --split-master live and persists on release", () => {
    split.initControls();
    const el = divider();
    const splitEl = document.querySelector(".split") as HTMLElement;
    // happy-dom yields a zero rect; give the controller real geometry.
    vi.spyOn(splitEl, "getBoundingClientRect").mockReturnValue({
      left: 0,
      width: 1000,
      top: 0,
      height: 300,
      right: 1000,
      bottom: 300,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);
    el.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 1, bubbles: true }),
    );
    el.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 1,
        clientX: 620,
        bubbles: true,
      }),
    );
    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("62%");
    el.dispatchEvent(
      new PointerEvent("pointerup", { pointerId: 1, bubbles: true }),
    );
    expect(localStorage.getItem(SPLIT_KEY)).toBe("62");
    expect(el.getAttribute("aria-valuenow")).toBe("62");
  });

  it("double-click resets to the 50/50 default", () => {
    split.initControls();
    localStorage.setItem(SPLIT_KEY, "62");
    divider().dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("");
    expect(localStorage.getItem(SPLIT_KEY)).toBeNull();
    expect(divider().getAttribute("aria-valuenow")).toBe("50");
  });

  it("ArrowRight widens the master pane, Enter resets", () => {
    split.initControls();
    const el = divider();
    const before = Number(el.getAttribute("aria-valuenow"));
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
    expect(Number(el.getAttribute("aria-valuenow"))).toBeGreaterThan(before);
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(Number(el.getAttribute("aria-valuenow"))).toBe(50);
  });

  it("ArrowLeft past the floor snaps into reading mode instead of clamping", () => {
    store.commit({ selected: { kind: "revision", id: REV }, open: true });
    split.initControls();
    const el = divider();
    el.setAttribute("aria-valuenow", "25"); // at the floor
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }),
    );
    expect(store.getState().reading).toBe(true);
  });

  it("dragging past the floor snaps into reading mode and ends the drag", () => {
    store.commit({ selected: { kind: "revision", id: REV }, open: true });
    split.initControls();
    const el = divider();
    const splitEl = document.querySelector(".split") as HTMLElement;
    vi.spyOn(splitEl, "getBoundingClientRect").mockReturnValue({
      left: 0,
      width: 1000,
      top: 0,
      height: 300,
      right: 1000,
      bottom: 300,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);
    el.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 1, bubbles: true }),
    );
    el.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 1,
        clientX: 40, // 4% — far past the 25% floor
        bubbles: true,
      }),
    );
    expect(store.getState().reading).toBe(true);
    expect(el.classList.contains("dragging")).toBe(false);
  });

  it("pointerdown focuses the divider so arrow keys work right after a click", () => {
    split.initControls();
    const el = divider();
    el.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 1, bubbles: true }),
    );
    expect(document.activeElement).toBe(el);
  });

  it("divider keys never leak to the global keyboard handler", () => {
    document.addEventListener("keydown", keyboard.onKey);
    store.commit({ selected: { kind: "revision", id: REV }, open: false });
    split.initControls();
    const el = divider();
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    // The divider reset ran; the Enter ladder did NOT (the pane stayed closed).
    expect(store.getState().open).toBe(false);
    expect(el.getAttribute("aria-valuenow")).toBe("50");
    document.removeEventListener("keydown", keyboard.onKey);
  });
});
