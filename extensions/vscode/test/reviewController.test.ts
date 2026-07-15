// @vitest-environment happy-dom

import { beforeEach, describe, expect, it, vi } from "vitest";
import snapshotFixture from "../../../src/cli/inspect/web/test/fixtures/snapshot.json";
import type { ReviewSnapshotDoc } from "../src/cli";
import type { DiffRenderData } from "../src/diffDataSource";
import {
  focusCandidateIds,
  ReviewWebviewController,
} from "../src/webview/reviewController";

beforeEach(() => {
  document.body.innerHTML = '<main id="review-root"></main>';
  HTMLElement.prototype.scrollIntoView = vi.fn();
});

describe("ReviewWebviewController", () => {
  it("renders the accordion and navigator, then restores same-location state", () => {
    let saved: unknown;
    const api = {
      getState: vi.fn(() => saved),
      setState: vi.fn((state: unknown) => {
        saved = state;
      }),
    };
    const root = requiredRoot();
    const controller = new ReviewWebviewController(root, api);
    controller.render(renderData("rev:one"));

    expect(root.querySelectorAll(".dfile").length).toBeGreaterThan(0);
    expect(root.querySelectorAll(".diff-nav-file").length).toBeGreaterThan(0);
    const input = root.querySelector<HTMLInputElement>("#diff-file-query");
    expect(input).not.toBeNull();
    if (input) {
      input.value = "path:lib";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }
    const header = root.querySelector<HTMLElement>(".dfile-head");
    const before = header?.getAttribute("aria-expanded");
    header?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(header?.getAttribute("aria-expanded")).not.toBe(before);
    expect(api.setState).toHaveBeenCalled();

    const restoredRoot = document.createElement("main");
    document.body.append(restoredRoot);
    const restored = new ReviewWebviewController(restoredRoot, api);
    restored.render(renderData("rev:one"));
    expect(
      restoredRoot.querySelector<HTMLInputElement>("#diff-file-query")?.value,
    ).toBe("path:lib");
  });

  it("resets controller state before a different revision renders", () => {
    let saved: unknown;
    const api = {
      getState: () => saved,
      setState: (state: unknown) => {
        saved = state;
      },
    };
    const controller = new ReviewWebviewController(requiredRoot(), api);
    controller.render(renderData("rev:one"));
    const first = document.querySelector<HTMLInputElement>("#diff-file-query");
    if (first) {
      first.value = "has:facts";
      first.dispatchEvent(new Event("input", { bubbles: true }));
    }

    controller.render(renderData("rev:two"));
    expect(
      document.querySelector<HTMLInputElement>("#diff-file-query")?.value,
    ).toBe("");
  });

  it("keeps n/p/[/] inside the page focus silo but leaves inputs alone", () => {
    const controller = new ReviewWebviewController(requiredRoot(), {
      getState: () => undefined,
      setState: vi.fn(),
    });
    controller.render(renderData("rev:one"));

    const nextFact = keydown(document.body, "n");
    expect(nextFact.defaultPrevented).toBe(true);
    expect(document.querySelector(".review-current.anno")).not.toBeNull();
    const previousFact = keydown(document.body, "p");
    expect(previousFact.defaultPrevented).toBe(true);
    expect(document.querySelector(".review-current.anno")).not.toBeNull();
    const nextChange = keydown(document.body, "]");
    expect(nextChange.defaultPrevented).toBe(true);
    expect(document.querySelector(".review-current.dhunk")).not.toBeNull();
    const previousChange = keydown(document.body, "[");
    expect(previousChange.defaultPrevented).toBe(true);
    expect(document.querySelector(".review-current.dhunk")).not.toBeNull();

    const input = document.querySelector<HTMLInputElement>("#diff-file-query");
    expect(input).not.toBeNull();
    const typing = input ? keydown(input, "n") : new KeyboardEvent("keydown");
    expect(typing.defaultPrevented).toBe(false);
  });

  it("leaves modified page keys for VS Code shortcuts", () => {
    const controller = new ReviewWebviewController(requiredRoot(), {
      getState: () => undefined,
      setState: vi.fn(),
    });
    controller.render(renderData("rev:one"));

    const shortcuts = [
      ["Cmd-N", "n", { metaKey: true }],
      ["Cmd-Shift-P", "p", { metaKey: true, shiftKey: true }],
      ["Cmd-Shift-[", "[", { metaKey: true, shiftKey: true }],
      ["Cmd-Shift-]", "]", { metaKey: true, shiftKey: true }],
      ["Ctrl-N", "n", { ctrlKey: true }],
      ["Alt-P", "p", { altKey: true }],
      ["Shift-[", "[", { shiftKey: true }],
    ] as const;
    for (const [label, key, modifiers] of shortcuts) {
      const observedByHost = vi.fn();
      window.addEventListener("keydown", observedByHost);
      const event = keydown(document.body, key, modifiers);
      window.removeEventListener("keydown", observedByHost);

      expect(event.defaultPrevented, label).toBe(false);
      expect(observedByHost, label).toHaveBeenCalledOnce();
      expect(document.querySelector(".review-current"), label).toBeNull();
    }
  });

  it.each([
    "Enter",
    " ",
  ])("activates an annotated row with the %s key", (key) => {
    const root = requiredRoot();
    const controller = new ReviewWebviewController(root, {
      getState: () => undefined,
      setState: vi.fn(),
    });
    controller.render(renderData("rev:one"));

    const row = root.querySelector<HTMLElement>(
      '.drow-noted[data-anno="obs:sha256:one"]',
    );
    expect(row).not.toBeNull();
    const activation = row ? keydown(row, key) : new KeyboardEvent("keydown");

    expect(activation.defaultPrevented).toBe(true);
    expect(
      root.querySelector('.review-focus[data-anno="obs:sha256:one"]'),
    ).not.toBeNull();
  });

  it("maps an attention item focus to its anchored review fact", () => {
    const controller = new ReviewWebviewController(requiredRoot(), {
      getState: () => undefined,
      setState: vi.fn(),
    });
    const data = {
      ...renderData("rev:one"),
      annotations: [
        {
          id: "request:sha256:one",
          kind: "input-request",
          title: "Choose",
          track: "agent:review",
          target: { kind: "revision", revisionId: "rev:one" },
        },
      ],
    };
    controller.render(data, {
      kind: "attention",
      id: "open_input_request:request:sha256:one",
    });

    expect(focusCandidateIds("open_input_request:request:sha256:one")).toEqual([
      "open_input_request:request:sha256:one",
      "request:sha256:one",
    ]);
    expect(
      document.querySelector<HTMLElement>(".review-focus[data-anno]")?.dataset
        .anno,
    ).toBe("request:sha256:one");
  });

  it("emits a typed source target from a rendered snapshot row", () => {
    const postMessage = vi.fn();
    const root = requiredRoot();
    const controller = new ReviewWebviewController(root, {
      getState: () => undefined,
      setState: vi.fn(),
      postMessage,
    });
    controller.render(renderData("rev:one"));

    const action = root.querySelector<HTMLButtonElement>(
      '[data-open-source="true"][data-source-line="2"][data-source-side="new"]',
    );
    expect(action).not.toBeNull();
    action?.click();

    expect(postMessage).toHaveBeenCalledWith({
      type: "openSource",
      target: {
        filePath: "src/lib.rs",
        side: "new",
        startLine: 2,
        endLine: 2,
      },
    });
  });

  it("binds source actions when a collapsed file renders lazily", () => {
    const postMessage = vi.fn();
    const root = requiredRoot();
    const controller = new ReviewWebviewController(root, {
      getState: () => undefined,
      setState: vi.fn(),
      postMessage,
    });
    const data = { ...renderData("rev:one"), annotations: [] };
    const firstFile = data.artifact.snapshot.files[0];
    firstFile.is_binary = true;
    controller.render(data);

    expect(root.querySelector("[data-open-source]")).toBeNull();
    root.querySelector<HTMLElement>(".dfile-head")?.click();
    const action = root.querySelector<HTMLButtonElement>(
      '[data-open-source="true"][data-source-side="new"]',
    );
    expect(action).not.toBeNull();
    action?.click();
    expect(postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: "openSource" }),
    );
  });
});

function renderData(revisionId: string): DiffRenderData {
  return {
    revisionId,
    snapshotId: "obj:sha256:fixture",
    artifact: structuredClone(snapshotFixture) as ReviewSnapshotDoc,
    annotations: [
      {
        id: "obs:sha256:one",
        kind: "observation",
        title: "Observed",
        track: "agent:author",
        target: {
          kind: "range",
          revisionId,
          filePath: "src/lib.rs",
          side: "new",
          startLine: 2,
          endLine: 2,
        },
      },
    ],
  };
}

function requiredRoot(): HTMLElement {
  const root = document.querySelector<HTMLElement>("#review-root");
  if (!root) throw new Error("missing root");
  return root;
}

function keydown(
  target: EventTarget,
  key: string,
  modifiers: KeyboardEventInit = {},
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
    ...modifiers,
  });
  target.dispatchEvent(event);
  return event;
}
