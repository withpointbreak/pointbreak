import http from "node:http";
import https from "node:https";
import { type VersionDoc, verifyHandshake } from "./cli";

const REQUEST_TIMEOUT_MS = 1_000;
const MAX_RESPONSE_BYTES = 1024 * 1024;

export type FetchFn = (
  url: URL,
  init: { headers: Record<string, string>; signal: AbortSignal },
) => Promise<{ status: number; text(): Promise<string> }>;

export interface InspectIdentity {
  readonly storeIdentity: string;
  readonly contextIdentity: string;
}

export type InspectClientErrorKind =
  | "unauthorized"
  | "unavailable"
  | "protocol"
  | "incompatible"
  | "mismatch";

export class InspectClientError extends Error {
  constructor(readonly kind: InspectClientErrorKind) {
    super(errorMessage(kind));
    this.name = "InspectClientError";
  }
}

/** Credential-holding request boundary for authenticated inspect handshakes. */
export class InspectClient {
  readonly #origin: URL;
  readonly #token: string;
  readonly #fetch: FetchFn;
  readonly #timeoutMs: number;

  constructor(
    origin: string,
    token: string,
    fetch: FetchFn = defaultFetch,
    timeoutMs = REQUEST_TIMEOUT_MS,
  ) {
    const parsed = new URL(origin);
    if (
      (parsed.protocol !== "http:" && parsed.protocol !== "https:") ||
      parsed.username ||
      parsed.password ||
      parsed.pathname !== "/" ||
      parsed.search ||
      parsed.hash
    ) {
      throw new InspectClientError("protocol");
    }
    this.#origin = new URL(parsed.origin);
    this.#token = token;
    this.#fetch = fetch;
    this.#timeoutMs = timeoutMs;
  }

  async verify(identity: InspectIdentity): Promise<void> {
    await this.verifyVersion();
    await this.verifyIdentity(identity);
  }

  async verifyVersion(): Promise<VersionDoc> {
    const document = await this.document("/api/version");
    if (!isVersionDocument(document)) {
      throw new InspectClientError("protocol");
    }
    const result = verifyHandshake(document);
    if (!result.ok) {
      throw new InspectClientError("incompatible");
    }
    return document;
  }

  async verifyIdentity(expected: InspectIdentity): Promise<InspectIdentity> {
    const document = await this.document("/api/identity");
    if (
      !isObject(document) ||
      document.schema !== "pointbreak.inspect-identity" ||
      typeof document.storeIdentity !== "string" ||
      typeof document.contextIdentity !== "string"
    ) {
      throw new InspectClientError("protocol");
    }
    if (
      document.storeIdentity !== expected.storeIdentity ||
      document.contextIdentity !== expected.contextIdentity
    ) {
      throw new InspectClientError("mismatch");
    }
    return {
      storeIdentity: document.storeIdentity,
      contextIdentity: document.contextIdentity,
    };
  }

  private async document(path: string): Promise<unknown> {
    const url = new URL(path, this.#origin);
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.#timeoutMs);
    let response: Awaited<ReturnType<FetchFn>>;
    try {
      response = await this.#fetch(url, {
        headers: {
          Host: this.#origin.host,
          Authorization: `Bearer ${this.#token}`,
        },
        signal: controller.signal,
      });
    } catch {
      throw new InspectClientError("unavailable");
    } finally {
      clearTimeout(timeout);
    }

    if (response.status === 401) {
      throw new InspectClientError("unauthorized");
    }
    if (response.status < 200 || response.status >= 300) {
      throw new InspectClientError("protocol");
    }
    let body: string;
    try {
      body = await response.text();
      return JSON.parse(body);
    } catch {
      throw new InspectClientError("protocol");
    }
  }
}

function isVersionDocument(value: unknown): value is VersionDoc {
  return (
    isObject(value) &&
    value.schema === "pointbreak.version" &&
    value.version === 1 &&
    typeof value.cliVersion === "string" &&
    isObject(value.documents)
  );
}

function errorMessage(kind: InspectClientErrorKind): string {
  switch (kind) {
    case "unauthorized":
      return "Pointbreak Review authentication was rejected.";
    case "unavailable":
      return "Pointbreak Review is unavailable.";
    case "protocol":
      return "Pointbreak Review returned an invalid response.";
    case "incompatible":
      return "Pointbreak Review is incompatible with this extension.";
    case "mismatch":
      return "Pointbreak Review belongs to another review target.";
  }
}

const defaultFetch: FetchFn = (url, init) =>
  new Promise((resolve, reject) => {
    const transport = url.protocol === "https:" ? https : http;
    const request = transport.request(
      url,
      {
        method: "GET",
        headers: init.headers,
        signal: init.signal,
      },
      (response) => {
        const chunks: Buffer[] = [];
        let bytes = 0;
        response.on("data", (chunk: Buffer) => {
          bytes += chunk.length;
          if (bytes > MAX_RESPONSE_BYTES) {
            request.destroy(new Error("response too large"));
            return;
          }
          chunks.push(chunk);
        });
        response.once("end", () => {
          const body = Buffer.concat(chunks).toString("utf8");
          resolve({
            status: response.statusCode ?? 0,
            text: async () => body,
          });
        });
      },
    );
    request.once("error", reject);
    request.end();
  });

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
