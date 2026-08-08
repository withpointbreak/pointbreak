import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { promptForCredential } from "../src/auth";
import { installChangeInspectorInteraction } from "../src/change-inspector-interaction";
import type { ChangeInspectorSnapshot } from "../src/change-inspector-state";
import { mountInspectorDom, resetDom } from "./support/dom";

const lensSnapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route: { kind: "lens", lens: "changes", query: {} },
  selected: null,
  diagnostic: null,
});

const attentionSnapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route: { kind: "lens", lens: "attention", query: { q: "needs-review" } },
  selected: null,
  diagnostic: null,
});

const exactSnapshot = (
  changeId = "change:sha256:interaction-lifecycle",
  query: Record<string, string> = {},
): ChangeInspectorSnapshot => ({
  generation: null,
  route: {
    kind: "change",
    changeId,
    query,
  },
  selected: null,
  diagnostic: null,
});

const activeControllers: Array<{ stop(): void }> = [];

function install() {
  const navigate = vi.fn();
  const controller = installChangeInspectorInteraction({ navigate });
  activeControllers.push(controller);
  return { controller, navigate };
}

beforeEach(() => {
  localStorage.clear();
  mountInspectorDom();
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(() => true),
      }) as unknown as MediaQueryList,
  );
});

afterEach(() => {
  for (const controller of activeControllers.splice(0)) controller.stop();
  vi.restoreAllMocks();
  resetDom();
});

describe("Change Inspector interaction lifecycle", () => {
  it("leaves Enter on a focused native control instead of opening the local Change cursor", () => {
    const { controller, navigate } = install();
    const card = document.createElement("article");
    card.className = "unit-card";
    card.dataset.changeId = "change:sha256:selected";
    const exactPeer = document.createElement("button");
    exactPeer.type = "button";
    exactPeer.textContent = "open exact Revision";
    card.append(exactPeer);
    document.querySelector("#master")?.append(card);
    controller.sync(lensSnapshot());

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    exactPeer.focus();
    exactPeer.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(document.activeElement).toBe(exactPeer);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("supports pointer resizing and double-click reset on the active split divider", () => {
    const { controller } = install();
    controller.sync(exactSnapshot());
    const split = document.querySelector<HTMLElement>(".split");
    const divider = document.querySelector<HTMLElement>(".divider");
    if (!split || !divider) throw new Error("split controls missing");
    vi.spyOn(split, "getBoundingClientRect").mockReturnValue({
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

    divider.dispatchEvent(
      new PointerEvent("pointerdown", {
        pointerId: 9,
        pointerType: "mouse",
        button: 2,
        bubbles: true,
      }),
    );
    expect(divider.classList).not.toContain("dragging");
    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 1, bubbles: true }),
    );
    expect(document.activeElement).toBe(divider);
    divider.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 2,
        clientX: 700,
        bubbles: true,
      }),
    );
    divider.dispatchEvent(
      new PointerEvent("pointerup", { pointerId: 2, bubbles: true }),
    );
    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("");
    expect(divider.classList).toContain("dragging");
    divider.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 1,
        clientX: 620,
        bubbles: true,
      }),
    );
    divider.dispatchEvent(
      new PointerEvent("pointerup", { pointerId: 1, bubbles: true }),
    );

    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("62%");
    expect(divider.getAttribute("aria-valuenow")).toBe("62");
    expect(divider.classList).not.toContain("dragging");

    divider.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("");
    expect(divider.getAttribute("aria-valuenow")).toBe("50");
  });

  it("cleans divider pointer capture on cancellation, loss, and controller stop", () => {
    const { controller } = install();
    controller.sync(exactSnapshot());
    const divider = document.querySelector<HTMLElement>(".divider");
    if (!divider) throw new Error("divider missing");
    const releasePointerCapture = vi.fn();
    Object.defineProperties(divider, {
      setPointerCapture: { configurable: true, value: vi.fn() },
      hasPointerCapture: {
        configurable: true,
        value: vi.fn(() => true),
      },
      releasePointerCapture: {
        configurable: true,
        value: releasePointerCapture,
      },
    });

    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 3, bubbles: true }),
    );
    divider.dispatchEvent(
      new PointerEvent("pointercancel", { pointerId: 3, bubbles: true }),
    );
    expect(divider.classList).not.toContain("dragging");
    expect(releasePointerCapture).toHaveBeenLastCalledWith(3);

    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 4, bubbles: true }),
    );
    divider.dispatchEvent(
      new PointerEvent("lostpointercapture", {
        pointerId: 4,
        bubbles: true,
      }),
    );
    expect(divider.classList).not.toContain("dragging");

    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 5, bubbles: true }),
    );
    controller.stop();
    expect(divider.classList).not.toContain("dragging");
    expect(releasePointerCapture).toHaveBeenLastCalledWith(5);
  });

  it("settles reconnect Escape through auth and restores the prior focus", async () => {
    const { controller, navigate } = install();
    controller.sync(lensSnapshot());
    const opener = document.createElement("button");
    opener.textContent = "retry connection";
    document.querySelector("#master")?.append(opener);
    opener.focus();

    const pending = promptForCredential();
    const input = document.querySelector<HTMLInputElement>("#reconnect-input");
    const submit =
      document.querySelector<HTMLButtonElement>("#reconnect-submit");
    expect(document.activeElement).toBe(input);

    submit?.focus();
    submit?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "1", bubbles: true }),
    );
    expect(navigate).not.toHaveBeenCalled();

    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    submit?.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(input);

    input?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );

    await expect(pending).resolves.toBeNull();
    expect(
      document.querySelector("#reconnect-dialog")?.classList.contains("hidden"),
    ).toBe(true);
    expect(document.activeElement).toBe(opener);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("leaves reading mode before an exact route returns focus to its lens", () => {
    const { controller } = install();
    controller.sync(lensSnapshot());
    const opener = document.createElement("button");
    opener.textContent = "open change";
    document.querySelector("#master")?.append(opener);
    opener.focus();

    controller.sync(exactSnapshot());
    document.querySelector<HTMLButtonElement>("#detail-read")?.click();
    expect(document.querySelector(".split")?.classList).toContain("reading");

    controller.sync(lensSnapshot());

    const split = document.querySelector(".split");
    expect(split?.classList).not.toContain("reading");
    expect(split?.classList).toContain("split-closed");
    expect(document.querySelector<HTMLElement>("#detail")?.inert).toBe(true);
    expect(document.activeElement).toBe(opener);
  });

  it("restores retained detail focus when an exact route replaces focused detail DOM", () => {
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    history.pushState(null, "", "/#/changes/change%3Asha256%3Aone");
    controller.sync(exactSnapshot("change:sha256:one"));

    const oldDetailControl = document.createElement("button");
    oldDetailControl.textContent = "old exact action";
    document.querySelector("#detail-body")?.append(oldDetailControl);
    oldDetailControl.focus();
    expect(document.activeElement).toBe(oldDetailControl);

    history.pushState(null, "", "/#/changes/change%3Asha256%3Atwo");
    document.querySelector("#detail-body")?.replaceChildren();
    controller.sync(exactSnapshot("change:sha256:two"));

    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );
  });

  it("restores focus after a same-route detail refresh but not an unchanged paint", () => {
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    history.pushState(null, "", "/#/changes/change%3Asha256%3Asame");
    const exact = exactSnapshot("change:sha256:same");
    controller.sync(exact);

    const oldDetailControl = document.createElement("button");
    oldDetailControl.textContent = "generation one action";
    document.querySelector("#detail-body")?.append(oldDetailControl);
    oldDetailControl.focus();
    expect(document.activeElement).toBe(oldDetailControl);

    const nextGeneration = document.createElement("p");
    nextGeneration.textContent = "generation two exact detail";
    document.querySelector("#detail-body")?.replaceChildren(nextGeneration);
    controller.sync(exact);
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );

    const retainedFocus = document.querySelector<HTMLElement>("#filter-text");
    retainedFocus?.focus();
    controller.sync(exact);
    expect(document.activeElement).toBe(retainedFocus);
  });

  it("falls back to the active surface when a modal opener was detached", () => {
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    const lensOpener = document.createElement("button");
    document.querySelector("#master")?.append(lensOpener);
    lensOpener.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "?",
        bubbles: true,
        cancelable: true,
      }),
    );
    lensOpener.remove();
    document.querySelector<HTMLButtonElement>("#key-help-close")?.click();
    expect(document.activeElement).toBe(document.querySelector("#master"));

    history.pushState(null, "", "/#/changes/change%3Asha256%3Amodal");
    controller.sync(exactSnapshot("change:sha256:modal"));
    const exactOpener = document.createElement("button");
    document.querySelector("#detail-body")?.append(exactOpener);
    exactOpener.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    exactOpener.remove();
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );
  });

  it("returns through the exact history entry's originating lens", () => {
    const { controller, navigate } = install();
    history.replaceState(null, "", "/#/attention?q=needs-review");
    controller.sync(attentionSnapshot());
    history.pushState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aorigin?q=needs-review",
    );
    const exact = exactSnapshot("change:sha256:origin", {
      q: "needs-review",
    });
    controller.sync(exact);
    const exactHistoryState = history.state;

    // Visiting another lens must not mutate the return target already recorded
    // on the exact entry. This models Back -> another lens -> Forward.
    history.pushState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    history.replaceState(
      exactHistoryState,
      "",
      "/#/changes/change%3Asha256%3Aorigin?q=needs-review",
    );
    controller.sync(exact);
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(navigate).toHaveBeenLastCalledWith({
      kind: "lens",
      lens: "attention",
      query: { q: "needs-review" },
    });
  });

  it("uses Changes for a direct exact link after the prior composition stops", () => {
    const first = install();
    history.replaceState(null, "", "/#/attention?q=needs-review");
    first.controller.sync(attentionSnapshot());
    history.pushState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aprior?q=needs-review",
    );
    first.controller.sync(
      exactSnapshot("change:sha256:prior", { q: "needs-review" }),
    );
    first.controller.stop();
    document.querySelector<HTMLButtonElement>("#detail-close")?.click();
    expect(first.navigate).not.toHaveBeenCalled();

    history.replaceState(null, "", "/#/changes/change%3Asha256%3Adirect");
    const second = install();
    second.controller.sync(exactSnapshot("change:sha256:direct"));
    document.querySelector<HTMLButtonElement>("#detail-close")?.click();

    expect(second.navigate).toHaveBeenLastCalledWith({
      kind: "lens",
      lens: "changes",
      query: {},
    });
  });

  it("does not retain a stopped composition callback in module state", async () => {
    const source = await readFile(
      resolve(process.cwd(), "src/change-inspector-interaction.ts"),
      "utf8",
    );

    expect(source).not.toMatch(/let cleanup:\s*\(\(\) => void\)/);
    expect(source).not.toContain("cleanup?.()");
    expect(source).not.toContain("cleanup = stop");
    expect(source).not.toMatch(
      /^let (selectedChangeId|modalReturnFocus|detailReturnFocus|detailWasOpen|currentRoute|exactOriginLens|detailDomIdentity)/m,
    );
    expect(source).not.toContain("__pointbreakChangeRoute");
  });
});
