const SESSION_TOKEN_PREFIX = "pointbreak.inspect-token.v1:";
let credentialVersion = 0;

export interface CapabilityExtraction {
  token: string | null;
  cleanedHash: string;
}

export function sessionTokenKey(origin = location.origin): string {
  return `${SESSION_TOKEN_PREFIX}${origin}`;
}

export function getSessionToken(): string | null {
  return sessionStorage.getItem(sessionTokenKey());
}

export function setSessionToken(token: string): void {
  if (!token) throw new Error("invalid capability");
  sessionStorage.setItem(sessionTokenKey(), token);
  credentialVersion += 1;
}

export function sessionCredentialVersion(): number {
  return credentialVersion;
}

function decoded(value: string): string {
  try {
    return decodeURIComponent(value.replace(/\+/g, "%20"));
  } catch {
    throw new Error("invalid capability");
  }
}

/** Extract exactly one non-empty token while retaining every other fragment byte. */
export function extractCapability(hash: string): CapabilityExtraction {
  const prefixed = hash.startsWith("#") ? hash : `#${hash}`;
  const queryAt = prefixed.indexOf("?");
  if (queryAt < 0) return { token: null, cleanedHash: prefixed };

  const route = prefixed.slice(0, queryAt);
  const kept: string[] = [];
  const tokens: string[] = [];
  for (const pair of prefixed.slice(queryAt + 1).split("&")) {
    if (!pair) continue;
    const separator = pair.indexOf("=");
    const rawKey = separator < 0 ? pair : pair.slice(0, separator);
    const rawValue = separator < 0 ? "" : pair.slice(separator + 1);
    if (decoded(rawKey) === "token") tokens.push(decoded(rawValue));
    else kept.push(pair);
  }
  if (tokens.length > 1 || (tokens.length === 1 && !tokens[0])) {
    throw new Error("invalid capability");
  }
  return {
    token: tokens[0] ?? null,
    cleanedHash: kept.length ? `${route}?${kept.join("&")}` : route,
  };
}

/** Move a fragment capability to origin-scoped sessionStorage before routing. */
export function bootstrapCapability(): CapabilityExtraction {
  const result = extractCapability(location.hash);
  if (result.token !== null) {
    setSessionToken(result.token);
    history.replaceState(
      history.state,
      "",
      `${location.pathname}${location.search}${result.cleanedHash}`,
    );
  }
  return result;
}

export type ReconnectTarget =
  | { kind: "retry"; token: string }
  | { kind: "navigate"; url: string };

function isLoopbackLiteral(hostname: string): boolean {
  const unbracketed = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  if (unbracketed === "::1") return true;
  const octets = unbracketed.split(".");
  return (
    octets.length === 4 &&
    octets.every((octet) => /^\d+$/.test(octet) && Number(octet) <= 255) &&
    Number(octets[0]) === 127
  );
}

function routeWithToken(route: string, token: string): string {
  const cleaned = extractCapability(route).cleanedHash;
  const separator = cleaned.includes("?") ? "&" : "?";
  return `${cleaned}${separator}token=${encodeURIComponent(token)}`;
}

export function resolveReconnectInput(
  input: string,
  currentOrigin: string,
  currentRoute: string,
): ReconnectTarget {
  const value = input.trim();
  if (!value) throw new Error("invalid capability URL");
  if (!/^[a-z][a-z0-9+.-]*:/i.test(value)) {
    return { kind: "retry", token: value };
  }

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error("invalid capability URL");
  }
  if (
    url.protocol !== "http:" ||
    !isLoopbackLiteral(url.hostname) ||
    url.username ||
    url.password
  ) {
    throw new Error("invalid capability URL");
  }
  let extraction: CapabilityExtraction;
  try {
    extraction = extractCapability(url.hash);
  } catch {
    throw new Error("invalid capability URL");
  }
  if (!extraction.token) throw new Error("invalid capability URL");
  if (url.origin === currentOrigin) {
    return { kind: "retry", token: extraction.token };
  }
  return {
    kind: "navigate",
    url: `${url.origin}/${routeWithToken(currentRoute, extraction.token)}`,
  };
}

export interface AuthCoordinatorOptions {
  prompt: () => Promise<string | null>;
  navigate: (url: string) => void;
  currentOrigin: () => string;
  currentRoute: () => string;
}

export class AuthCoordinator {
  private recovery: Promise<boolean> | null = null;

  constructor(private readonly options: AuthCoordinatorOptions) {}

  recoverUnauthorized(): Promise<boolean> {
    if (this.recovery) return this.recovery;
    this.recovery = this.promptAndApply().finally(() => {
      this.recovery = null;
    });
    return this.recovery;
  }

  reconnect(): Promise<boolean> {
    return this.recoverUnauthorized();
  }

  private async promptAndApply(): Promise<boolean> {
    while (true) {
      const input = await this.options.prompt();
      if (input === null) {
        clearReconnectError();
        return false;
      }
      let target: ReconnectTarget;
      try {
        target = resolveReconnectInput(
          input,
          this.options.currentOrigin(),
          this.options.currentRoute(),
        );
      } catch {
        showReconnectError("Enter a token or an HTTP loopback capability URL.");
        continue;
      }
      clearReconnectError();
      if (target.kind === "retry") {
        setSessionToken(target.token);
        return true;
      }
      this.options.navigate(target.url);
      return false;
    }
  }
}

let installedCoordinator: AuthCoordinator | null = null;

export function installAuthCoordinator(coordinator: AuthCoordinator): void {
  installedCoordinator = coordinator;
}

/** Install the Inspector's standard loopback capability recovery policy. */
export function installDefaultAuthCoordinator(): AuthCoordinator {
  const coordinator = new AuthCoordinator({
    prompt: promptForCredential,
    navigate: (url) => location.replace(url),
    currentOrigin: () => location.origin,
    currentRoute: () => location.hash,
  });
  installAuthCoordinator(coordinator);
  return coordinator;
}

export function recoverUnauthorized(): Promise<boolean> {
  return installedCoordinator?.recoverUnauthorized() ?? Promise.resolve(false);
}

export function requestReconnect(): Promise<boolean> {
  return installedCoordinator?.reconnect() ?? Promise.resolve(false);
}

export function resetAuthForTests(): void {
  installedCoordinator = null;
  sessionStorage.removeItem(sessionTokenKey());
  credentialVersion += 1;
}

function showReconnectError(message: string): void {
  const error = document.querySelector<HTMLElement>("#reconnect-error");
  if (!error) return;
  error.textContent = message;
  error.classList.remove("hidden");
}

function clearReconnectError(): void {
  const error = document.querySelector<HTMLElement>("#reconnect-error");
  if (!error) return;
  error.textContent = "";
  error.classList.add("hidden");
}

/** Resolve with a credential without ever rendering or retaining its value. */
export function promptForCredential(): Promise<string | null> {
  const dialog = document.querySelector<HTMLElement>("#reconnect-dialog");
  const form = dialog?.querySelector<HTMLFormElement>("form");
  const input = document.querySelector<HTMLInputElement>("#reconnect-input");
  const cancel = document.querySelector<HTMLButtonElement>("#reconnect-cancel");
  if (!dialog || !form || !input || !cancel) return Promise.resolve(null);

  dialog.classList.remove("hidden");
  input.value = "";
  input.focus();

  return new Promise((resolve) => {
    let settled = false;
    const finish = (value: string | null) => {
      if (settled) return;
      settled = true;
      form.removeEventListener("submit", onSubmit);
      cancel.removeEventListener("click", onCancel);
      input.value = "";
      dialog.classList.add("hidden");
      resolve(value);
    };
    const onSubmit = (event: SubmitEvent) => {
      event.preventDefault();
      finish(input.value);
    };
    const onCancel = () => finish(null);
    form.addEventListener("submit", onSubmit);
    cancel.addEventListener("click", onCancel);
  });
}
