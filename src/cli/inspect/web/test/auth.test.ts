import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AuthCoordinator,
  bootstrapCapability,
  extractCapability,
  getSessionToken,
  promptForCredential,
  resolveReconnectInput,
  sessionTokenKey,
} from "../src/auth";
import { mountInspectorDom, resetDom } from "./support/dom";

const TOKEN = "opaque_test_capability_0123456789abcdef";

beforeEach(() => {
  sessionStorage.clear();
  history.replaceState(null, "", "/");
  mountInspectorDom();
});

afterEach(() => {
  sessionStorage.clear();
  resetDom();
  vi.restoreAllMocks();
});

describe("capability fragment bootstrap", () => {
  const cases = [
    ["#/timeline?token=TOKEN", "#/timeline"],
    [
      "#/timeline?q=error%20path&token=TOKEN&track=agent%3Acodex&order=asc",
      "#/timeline?q=error%20path&track=agent%3Acodex&order=asc",
    ],
    [
      "#/list?sort=activity&sel=rev%3Aone&token=TOKEN",
      "#/list?sort=activity&sel=rev%3Aone",
    ],
    [
      "#/attention?token=TOKEN&types=observation%2Cassessment",
      "#/attention?types=observation%2Cassessment",
    ],
    [
      "#/changes?limit=100&order=change_id_asc&token=TOKEN",
      "#/changes?limit=100&order=change_id_asc",
    ],
    [
      "#/revision/rev%3Aone?lens=list&token=TOKEN",
      "#/revision/rev%3Aone?lens=list",
    ],
    [
      "#/event/event%3Aone?token=TOKEN&focus=fact%3Aone",
      "#/event/event%3Aone?focus=fact%3Aone",
    ],
    [
      "#/revision/rev%3Aone/diff?focus=fact%3Aone&file=src%2Flib.rs&token=TOKEN&fq=has%3Afacts",
      "#/revision/rev%3Aone/diff?focus=fact%3Aone&file=src%2Flib.rs&fq=has%3Afacts",
    ],
  ] as const;

  it.each(cases)("removes only token from %s", (template, cleaned) => {
    const fragment = template.replace("TOKEN", TOKEN);
    const result = extractCapability(fragment);
    expect(result.token === TOKEN).toBe(true);
    expect(result.cleanedHash).toBe(cleaned);
    expect(result.cleanedHash.includes(TOKEN)).toBe(false);
  });

  it("stores the token under a versioned origin key and scrubs history", () => {
    const replace = vi.spyOn(history, "replaceState");
    history.replaceState(
      { retained: true },
      "",
      `/#/revision/rev%3Aone/diff?focus=fact%3Aone&token=${TOKEN}`,
    );
    replace.mockClear();

    const result = bootstrapCapability();

    expect(result.cleanedHash).toBe(
      "#/revision/rev%3Aone/diff?focus=fact%3Aone",
    );
    expect(sessionStorage.getItem(sessionTokenKey()) === TOKEN).toBe(true);
    expect(location.href.includes(TOKEN)).toBe(false);
    expect(replace).toHaveBeenCalledOnce();
    expect(JSON.stringify(history.state).includes(TOKEN)).toBe(false);
  });

  it("reuses the session token on reload without rewriting a clean route", () => {
    sessionStorage.setItem(sessionTokenKey(), TOKEN);
    history.replaceState(null, "", "/#/attention?types=observation");
    const replace = vi.spyOn(history, "replaceState");

    const result = bootstrapCapability();

    expect(result.token).toBeNull();
    expect(getSessionToken() === TOKEN).toBe(true);
    expect(replace).not.toHaveBeenCalled();
  });

  it("rejects empty or duplicate fragment credentials without echoing them", () => {
    for (const fragment of [
      "#/timeline?token=",
      `#/timeline?token=${TOKEN}&token=second`,
    ]) {
      try {
        extractCapability(fragment);
        throw new Error("expected invalid capability");
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        expect(message).toContain("invalid capability");
        expect(message.includes(TOKEN)).toBe(false);
      }
    }
  });
});

describe("reconnect target validation", () => {
  const currentOrigin = "http://127.0.0.1:7878";
  const currentRoute =
    "#/revision/rev%3Aone/diff?focus=fact%3Aone&file=src%2Flib.rs";

  it("uses raw and same-origin credentials without navigation", () => {
    const raw = resolveReconnectInput(TOKEN, currentOrigin, currentRoute);
    expect(raw.kind).toBe("retry");
    expect(raw.kind === "retry" && raw.token === TOKEN).toBe(true);
    const capability = resolveReconnectInput(
      `${currentOrigin}/#/attention?token=${TOKEN}`,
      currentOrigin,
      currentRoute,
    );
    expect(capability.kind).toBe("retry");
    expect(capability.kind === "retry" && capability.token === TOKEN).toBe(
      true,
    );
  });

  it("builds a different-loopback destination from the current clean route", () => {
    const result = resolveReconnectInput(
      `http://127.0.0.2:9000/#/attention?token=${TOKEN}`,
      currentOrigin,
      currentRoute,
    );
    expect(result.kind).toBe("navigate");
    if (result.kind === "navigate") {
      expect(
        result.url.startsWith(`http://127.0.0.2:9000/${currentRoute}`),
      ).toBe(true);
      expect(result.url.includes("#/attention")).toBe(false);
      expect(result.url.endsWith(`&token=${TOKEN}`)).toBe(true);
    }
  });

  it("accepts IPv4 loopback and bracketed IPv6 but rejects other schemes and hosts", () => {
    expect(
      resolveReconnectInput(
        `http://[::1]:9000/#/timeline?token=${TOKEN}`,
        currentOrigin,
        currentRoute,
      ).kind,
    ).toBe("navigate");

    for (const value of [
      `https://127.0.0.1:9000/#/timeline?token=${TOKEN}`,
      `http://localhost:9000/#/timeline?token=${TOKEN}`,
      `http://192.168.1.4:9000/#/timeline?token=${TOKEN}`,
      "http://127.0.0.2:9000/#/timeline",
    ]) {
      expect(() =>
        resolveReconnectInput(value, currentOrigin, currentRoute),
      ).toThrow("invalid capability URL");
    }
  });
});

describe("single-flight authentication recovery", () => {
  it("shares one prompt across concurrent unauthorized requests", async () => {
    let prompts = 0;
    let resolvePrompt!: (value: string | null) => void;
    const prompt = () => {
      prompts += 1;
      return new Promise<string | null>((resolve) => {
        resolvePrompt = resolve;
      });
    };
    const navigate = vi.fn();
    const coordinator = new AuthCoordinator({
      prompt,
      navigate,
      currentOrigin: () => "http://127.0.0.1:7878",
      currentRoute: () => "#/timeline",
    });

    const first = coordinator.recoverUnauthorized();
    const second = coordinator.recoverUnauthorized();
    expect(prompts).toBe(1);
    resolvePrompt(TOKEN);

    await expect(Promise.all([first, second])).resolves.toEqual([true, true]);
    expect(getSessionToken() === TOKEN).toBe(true);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("cancel preserves route and session state", async () => {
    history.replaceState(null, "", "/#/list?sort=activity");
    const navigate = vi.fn();
    const coordinator = new AuthCoordinator({
      prompt: async () => null,
      navigate,
      currentOrigin: () => location.origin,
      currentRoute: () => location.hash,
    });

    await expect(coordinator.recoverUnauthorized()).resolves.toBe(false);
    expect(location.hash).toBe("#/list?sort=activity");
    expect(getSessionToken()).toBeNull();
    expect(navigate).not.toHaveBeenCalled();
  });

  it("uses a password input and clears it after submission", async () => {
    const pending = promptForCredential();
    const input = document.querySelector<HTMLInputElement>("#reconnect-input");
    expect(input?.type).toBe("password");
    if (input) input.value = TOKEN;
    document.querySelector<HTMLButtonElement>("#reconnect-submit")?.click();

    const submitted = await pending;
    expect(submitted === TOKEN).toBe(true);
    expect(input?.value).toBe("");
    expect(document.body.textContent?.includes(TOKEN)).toBe(false);
  });

  it("submits the credential through the reconnect form", async () => {
    const pending = promptForCredential();
    const input = document.querySelector<HTMLInputElement>("#reconnect-input");
    const submit =
      document.querySelector<HTMLButtonElement>("#reconnect-submit");
    expect(input?.form).not.toBeNull();
    expect(submit?.type).toBe("submit");
    if (input) input.value = TOKEN;
    input?.form?.requestSubmit();

    await expect(pending).resolves.toBe(TOKEN);
  });

  it("keeps recovery open after a generic invalid-input error", async () => {
    const coordinator = new AuthCoordinator({
      prompt: promptForCredential,
      navigate: vi.fn(),
      currentOrigin: () => "http://127.0.0.1:7878",
      currentRoute: () => "#/timeline",
    });
    const recovered = coordinator.reconnect();
    const input = document.querySelector<HTMLInputElement>("#reconnect-input");
    const submit =
      document.querySelector<HTMLButtonElement>("#reconnect-submit");
    if (input) input.value = "https://example.test/#/timeline?token=not-shown";
    submit?.click();
    await Promise.resolve();

    const error = document.querySelector<HTMLElement>("#reconnect-error");
    expect(error?.textContent).toBe(
      "Enter a token or an HTTP loopback capability URL.",
    );
    expect(document.body.textContent?.includes("not-shown")).toBe(false);
    expect(
      document.querySelector("#reconnect-dialog")?.classList.contains("hidden"),
    ).toBe(false);

    if (input) input.value = TOKEN;
    submit?.click();
    await expect(recovered).resolves.toBe(true);
    expect(getSessionToken() === TOKEN).toBe(true);
  });
});
