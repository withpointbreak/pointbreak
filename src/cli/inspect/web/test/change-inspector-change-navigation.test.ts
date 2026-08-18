import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installChangeInspectorInteraction } from "../src/change-inspector-interaction";
import { renderChangeInspectorRefusal } from "../src/change-inspector-render";
import {
  type ChangeInspectorRoute,
  formatChangeInspectorRoute,
} from "../src/change-inspector-router";
import type { ChangeInspectorSnapshot } from "../src/change-inspector-state";
import { mountInspectorDom, resetDom } from "./support/dom";

const LENSES = ["changes", "attention"] as const;

type ChangeLens = (typeof LENSES)[number];
type LensRoute = Extract<ChangeInspectorRoute, { kind: "lens" }>;

const activeControllers: Array<{ stop(): void }> = [];

function lensRoute(lens: ChangeLens, after?: string): LensRoute {
  return {
    kind: "lens",
    lens,
    query: {
      q: `${lens}-filter`,
      ...(after === undefined ? {} : { after }),
    },
  };
}

function snapshot(route: LensRoute): ChangeInspectorSnapshot {
  return {
    generation: null,
    route,
    selected: null,
    diagnostic: null,
  };
}

function install() {
  const navigate = vi.fn();
  const replace = vi.fn();
  const controller = installChangeInspectorInteraction({ navigate, replace });
  activeControllers.push(controller);
  return { controller, navigate, replace };
}

function mountCards(changeIds: readonly string[]) {
  const list = document.createElement("section");
  list.className = "units";
  const peers = vi.fn();
  const primary = changeIds.map((changeId) => {
    const card = document.createElement("article");
    card.className = "unit-card";
    card.dataset.changeId = changeId;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "change-card-primary";
    button.textContent = `Review ${changeId}`;
    const peer = document.createElement("button");
    peer.type = "button";
    peer.textContent = `Open explicit peer for ${changeId}`;
    peer.addEventListener("click", peers);
    const heading = document.createElement("h3");
    heading.className = "change-card-heading";
    heading.append(button);
    card.append(heading, peer);
    list.append(card);
    return button;
  });
  document.querySelector("#master")?.replaceChildren(list);
  return { list, primary, peers };
}

function mountPager(
  list: HTMLElement,
  direction: "previous" | "next" | "last",
  target: LensRoute,
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.dataset.changePage = direction;
  button.dataset.changeTargetRoute = formatChangeInspectorRoute(target);
  button.textContent = `${direction} page`;
  list.append(button);
  return button;
}

function press(target: HTMLElement, key: string): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
  });
  target.dispatchEvent(event);
  return event;
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

describe.each(LENSES)("%s Change navigation", (lens) => {
  it.each([
    "j",
    "f",
    "d",
  ])("follows only the mounted next capability from a loaded tail with %s", (key) => {
    const { controller, navigate } = install();
    const source = lensRoute(lens, "opaque-source-tail");
    const destination = lensRoute(lens, "opaque-signed-next");
    const sourceCards = mountCards([
      `change:${lens}:one`,
      `change:${lens}:tail`,
    ]);
    mountPager(sourceCards.list, "next", destination);
    controller.sync(snapshot(source));
    const tail = sourceCards.primary.at(-1);
    if (!tail) throw new Error("missing tail Change primary");
    tail.focus();

    const event = press(tail, key);

    expect(event.defaultPrevented).toBe(true);
    expect(navigate).toHaveBeenCalledExactlyOnceWith(destination);
    expect(sourceCards.peers).not.toHaveBeenCalled();

    const destinationCards = mountCards([
      `change:${lens}:next-first`,
      `change:${lens}:next-last`,
    ]);
    controller.sync(snapshot(destination));

    expect(destinationCards.primary[0]?.tabIndex).toBe(0);
    expect(destinationCards.primary[1]?.tabIndex).toBe(-1);
    expect(document.activeElement).toBe(destinationCards.primary[0]);
    expect(destinationCards.peers).not.toHaveBeenCalled();
  });

  it.each([
    "k",
    "b",
    "u",
  ])("follows only the mounted previous capability from a loaded head with %s", (key) => {
    const { controller, navigate } = install();
    const source = lensRoute(lens, "opaque-source-head");
    const destination = lensRoute(lens, "opaque-signed-previous");
    const sourceCards = mountCards([
      `change:${lens}:head`,
      `change:${lens}:two`,
    ]);
    mountPager(sourceCards.list, "previous", destination);
    controller.sync(snapshot(source));
    const head = sourceCards.primary[0];
    if (!head) throw new Error("missing head Change primary");
    head.focus();

    const event = press(head, key);

    expect(event.defaultPrevented).toBe(true);
    expect(navigate).toHaveBeenCalledExactlyOnceWith(destination);
    expect(sourceCards.peers).not.toHaveBeenCalled();

    const destinationCards = mountCards([
      `change:${lens}:previous-first`,
      `change:${lens}:previous-last`,
    ]);
    controller.sync(snapshot(destination));

    const last = destinationCards.primary.at(-1);
    expect(last?.tabIndex).toBe(0);
    expect(document.activeElement).toBe(last);
    expect(destinationCards.peers).not.toHaveBeenCalled();
  });

  it("returns to the first Change page from a continuation and follows only mounted last", () => {
    const { controller, navigate } = install();
    const continuation = lensRoute(lens, "opaque-middle");
    const last = lensRoute(lens, "opaque-signed-last");
    const cards = mountCards([`change:${lens}:one`, `change:${lens}:two`]);
    mountPager(cards.list, "last", last);
    controller.sync(snapshot(continuation));
    const second = cards.primary[1];
    if (!second) throw new Error("missing Change primary");
    second.focus();

    const firstEvent = press(second, "g");
    expect(firstEvent.defaultPrevented).toBe(true);
    expect(navigate).toHaveBeenCalledExactlyOnceWith({
      kind: "lens",
      lens,
      query: { q: `${lens}-filter`, after: undefined },
    });

    navigate.mockClear();
    const lastEvent = press(second, "G");
    expect(lastEvent.defaultPrevented).toBe(true);
    expect(navigate).toHaveBeenCalledExactlyOnceWith(last);
    expect(cards.peers).not.toHaveBeenCalled();
  });

  it("does not steal focus changed deliberately while a signed page is loading", () => {
    const { controller } = install();
    const source = lensRoute(lens, "opaque-source-tail");
    const destination = lensRoute(lens, "opaque-signed-next");
    const sourceCards = mountCards([
      `change:${lens}:one`,
      `change:${lens}:tail`,
    ]);
    mountPager(sourceCards.list, "next", destination);
    controller.sync(snapshot(source));
    const tail = sourceCards.primary.at(-1);
    const laterFocus =
      document.querySelector<HTMLButtonElement>("#view-toggle");
    if (!tail || !laterFocus) throw new Error("missing focus-race fixture");
    tail.focus();
    press(tail, "f");
    laterFocus.focus();

    const destinationCards = mountCards([
      `change:${lens}:next-first`,
      `change:${lens}:next-last`,
    ]);
    controller.sync(snapshot(destination));

    expect(document.activeElement).toBe(laterFocus);
    expect(
      destinationCards.primary[0]
        ?.closest(".unit-card")
        ?.getAttribute("aria-current"),
    ).toBe("true");
  });

  it("clears pending focus when a signed page is refused", async () => {
    const { controller } = install();
    const source = lensRoute(lens, "opaque-source-tail");
    const destination = lensRoute(lens, "opaque-signed-next");
    const sourceCards = mountCards([
      `change:${lens}:one`,
      `change:${lens}:tail`,
    ]);
    mountPager(sourceCards.list, "next", destination);
    controller.sync(snapshot(source));
    const tail = sourceCards.primary.at(-1);
    if (!tail) throw new Error("missing refusal fixture");
    tail.focus();
    press(tail, "f");

    renderChangeInspectorRefusal(new Error("signed page refused"));
    await vi.waitFor(() =>
      expect(document.querySelector("#error")?.classList).not.toContain(
        "hidden",
      ),
    );
    const destinationCards = mountCards([
      `change:${lens}:next-first`,
      `change:${lens}:next-last`,
    ]);
    controller.sync(snapshot(destination));

    expect(document.activeElement).not.toBe(destinationCards.primary[0]);
    expect(
      document.querySelector(".unit-card[aria-current='true']"),
    ).toBeNull();
  });

  it("leaves page keys inert without a mounted capability", () => {
    const { controller, navigate } = install();
    const route = lensRoute(lens, "opaque-no-capability");
    const cards = mountCards([`change:${lens}:one`, `change:${lens}:two`]);
    controller.sync(snapshot(route));
    const first = cards.primary[0];
    const last = cards.primary[1];
    if (!first || !last) throw new Error("missing Change primary");

    for (const [target, key] of [
      [last, "f"],
      [last, "d"],
      [last, "j"],
      [first, "b"],
      [first, "u"],
      [first, "k"],
      [first, "G"],
    ] as const) {
      target.focus();
      const event = press(target, key);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(navigate).not.toHaveBeenCalled();
    expect(cards.peers).not.toHaveBeenCalled();
  });
});

it("keeps Changes and Attention cursors independent without activating peers", () => {
  const { controller } = install();
  const changes = lensRoute("changes");
  const changesCards = mountCards(["change:changes:one", "change:changes:two"]);
  controller.sync(snapshot(changes));
  const changesSecond = changesCards.primary[1];
  if (!changesSecond) throw new Error("missing Changes primary");
  changesSecond.focus();

  const attention = lensRoute("attention");
  const attentionCards = mountCards([
    "change:attention:one",
    "change:attention:two",
  ]);
  controller.sync(snapshot(attention));
  const attentionFirst = attentionCards.primary[0];
  if (!attentionFirst) throw new Error("missing Attention primary");
  attentionFirst.focus();

  const restoredChanges = mountCards([
    "change:changes:one",
    "change:changes:two",
  ]);
  controller.sync(snapshot(changes));

  expect(restoredChanges.primary[0]?.tabIndex).toBe(-1);
  expect(restoredChanges.primary[1]?.tabIndex).toBe(0);
  expect(changesCards.peers).not.toHaveBeenCalled();
  expect(attentionCards.peers).not.toHaveBeenCalled();
  expect(restoredChanges.peers).not.toHaveBeenCalled();
});
