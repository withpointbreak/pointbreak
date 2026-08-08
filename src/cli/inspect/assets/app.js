"use strict";
(() => {
  var __defProp = Object.defineProperty;
  var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

  // src/auth.ts
  var SESSION_TOKEN_PREFIX = "pointbreak.inspect-token.v1:";
  var credentialVersion = 0;
  function sessionTokenKey(origin = location.origin) {
    return `${SESSION_TOKEN_PREFIX}${origin}`;
  }
  __name(sessionTokenKey, "sessionTokenKey");
  function getSessionToken() {
    return sessionStorage.getItem(sessionTokenKey());
  }
  __name(getSessionToken, "getSessionToken");
  function setSessionToken(token) {
    if (!token) throw new Error("invalid capability");
    sessionStorage.setItem(sessionTokenKey(), token);
    credentialVersion += 1;
  }
  __name(setSessionToken, "setSessionToken");
  function sessionCredentialVersion() {
    return credentialVersion;
  }
  __name(sessionCredentialVersion, "sessionCredentialVersion");
  function decoded(value) {
    try {
      return decodeURIComponent(value.replace(/\+/g, "%20"));
    } catch {
      throw new Error("invalid capability");
    }
  }
  __name(decoded, "decoded");
  function extractCapability(hash) {
    const prefixed = hash.startsWith("#") ? hash : `#${hash}`;
    const queryAt = prefixed.indexOf("?");
    if (queryAt < 0) return { token: null, cleanedHash: prefixed };
    const route = prefixed.slice(0, queryAt);
    const kept = [];
    const tokens = [];
    for (const pair of prefixed.slice(queryAt + 1).split("&")) {
      if (!pair) continue;
      const separator = pair.indexOf("=");
      const rawKey = separator < 0 ? pair : pair.slice(0, separator);
      const rawValue = separator < 0 ? "" : pair.slice(separator + 1);
      if (decoded(rawKey) === "token") tokens.push(decoded(rawValue));
      else kept.push(pair);
    }
    if (tokens.length > 1 || tokens.length === 1 && !tokens[0]) {
      throw new Error("invalid capability");
    }
    return {
      token: tokens[0] ?? null,
      cleanedHash: kept.length ? `${route}?${kept.join("&")}` : route
    };
  }
  __name(extractCapability, "extractCapability");
  function bootstrapCapability() {
    const result = extractCapability(location.hash);
    if (result.token !== null) {
      setSessionToken(result.token);
      history.replaceState(
        history.state,
        "",
        `${location.pathname}${location.search}${result.cleanedHash}`
      );
    }
    return result;
  }
  __name(bootstrapCapability, "bootstrapCapability");
  function isLoopbackLiteral(hostname) {
    const unbracketed = hostname.replace(/^\[|\]$/g, "").toLowerCase();
    if (unbracketed === "::1") return true;
    const octets = unbracketed.split(".");
    return octets.length === 4 && octets.every((octet) => /^\d+$/.test(octet) && Number(octet) <= 255) && Number(octets[0]) === 127;
  }
  __name(isLoopbackLiteral, "isLoopbackLiteral");
  function routeWithToken(route, token) {
    const cleaned = extractCapability(route).cleanedHash;
    const separator = cleaned.includes("?") ? "&" : "?";
    return `${cleaned}${separator}token=${encodeURIComponent(token)}`;
  }
  __name(routeWithToken, "routeWithToken");
  function resolveReconnectInput(input, currentOrigin, currentRoute2) {
    const value = input.trim();
    if (!value) throw new Error("invalid capability URL");
    if (!/^[a-z][a-z0-9+.-]*:/i.test(value)) {
      return { kind: "retry", token: value };
    }
    let url;
    try {
      url = new URL(value);
    } catch {
      throw new Error("invalid capability URL");
    }
    if (url.protocol !== "http:" || !isLoopbackLiteral(url.hostname) || url.username || url.password) {
      throw new Error("invalid capability URL");
    }
    let extraction;
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
      url: `${url.origin}/${routeWithToken(currentRoute2, extraction.token)}`
    };
  }
  __name(resolveReconnectInput, "resolveReconnectInput");
  var AuthCoordinator = class {
    constructor(options) {
      this.options = options;
    }
    options;
    static {
      __name(this, "AuthCoordinator");
    }
    recovery = null;
    recoverUnauthorized() {
      if (this.recovery) return this.recovery;
      this.recovery = this.promptAndApply().finally(() => {
        this.recovery = null;
      });
      return this.recovery;
    }
    reconnect() {
      return this.recoverUnauthorized();
    }
    async promptAndApply() {
      while (true) {
        const input = await this.options.prompt();
        if (input === null) {
          clearReconnectError();
          return false;
        }
        let target;
        try {
          target = resolveReconnectInput(
            input,
            this.options.currentOrigin(),
            this.options.currentRoute()
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
  };
  var installedCoordinator = null;
  function installAuthCoordinator(coordinator) {
    installedCoordinator = coordinator;
  }
  __name(installAuthCoordinator, "installAuthCoordinator");
  function installDefaultAuthCoordinator() {
    const coordinator = new AuthCoordinator({
      prompt: promptForCredential,
      navigate: /* @__PURE__ */ __name((url) => location.replace(url), "navigate"),
      currentOrigin: /* @__PURE__ */ __name(() => location.origin, "currentOrigin"),
      currentRoute: /* @__PURE__ */ __name(() => location.hash, "currentRoute")
    });
    installAuthCoordinator(coordinator);
    return coordinator;
  }
  __name(installDefaultAuthCoordinator, "installDefaultAuthCoordinator");
  function recoverUnauthorized() {
    return installedCoordinator?.recoverUnauthorized() ?? Promise.resolve(false);
  }
  __name(recoverUnauthorized, "recoverUnauthorized");
  function requestReconnect() {
    return installedCoordinator?.reconnect() ?? Promise.resolve(false);
  }
  __name(requestReconnect, "requestReconnect");
  function showReconnectError(message2) {
    const error = document.querySelector("#reconnect-error");
    if (!error) return;
    error.textContent = message2;
    error.classList.remove("hidden");
  }
  __name(showReconnectError, "showReconnectError");
  function clearReconnectError() {
    const error = document.querySelector("#reconnect-error");
    if (!error) return;
    error.textContent = "";
    error.classList.add("hidden");
  }
  __name(clearReconnectError, "clearReconnectError");
  function promptForCredential() {
    const dialog = document.querySelector("#reconnect-dialog");
    const form = dialog?.querySelector("form");
    const input = document.querySelector("#reconnect-input");
    const cancel = document.querySelector("#reconnect-cancel");
    if (!dialog || !form || !input || !cancel) return Promise.resolve(null);
    const returnFocus = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
    dialog.classList.remove("hidden");
    input.value = "";
    input.focus();
    return new Promise((resolve) => {
      let settled = false;
      const finish = /* @__PURE__ */ __name((value) => {
        if (settled) return;
        settled = true;
        form.removeEventListener("submit", onSubmit);
        cancel.removeEventListener("click", onCancel);
        dialog.removeEventListener("keydown", onKeyDown);
        input.value = "";
        dialog.classList.add("hidden");
        if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
        resolve(value);
      }, "finish");
      const onSubmit = /* @__PURE__ */ __name((event) => {
        event.preventDefault();
        finish(input.value);
      }, "onSubmit");
      const onCancel = /* @__PURE__ */ __name(() => finish(null), "onCancel");
      const onKeyDown = /* @__PURE__ */ __name((event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          finish(null);
          return;
        }
        if (event.key !== "Tab") return;
        const stops = Array.from(
          dialog.querySelectorAll(
            "button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])"
          )
        );
        const first = stops[0];
        const last = stops.at(-1);
        if (!first || !last) return;
        const active2 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        if (event.shiftKey && (active2 === first || !dialog.contains(active2))) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && (active2 === last || !dialog.contains(active2))) {
          event.preventDefault();
          first.focus();
        }
      }, "onKeyDown");
      form.addEventListener("submit", onSubmit);
      cancel.addEventListener("click", onCancel);
      dialog.addEventListener("keydown", onKeyDown);
    });
  }
  __name(promptForCredential, "promptForCredential");

  // src/connection.ts
  var snapshot = {
    connection: "connecting",
    refresh: "idle"
  };
  function connectionPresentation(state) {
    const refreshLabel = state.refresh === "degraded" ? "response error" : state.refresh;
    switch (state.connection) {
      case "unauthorized":
        return {
          serverLabel: "local server",
          connectionLabel: "authentication required",
          refreshLabel,
          action: "Reconnect",
          canConnectAnother: false
        };
      case "unreachable":
        return {
          serverLabel: "local server",
          connectionLabel: "server unavailable",
          refreshLabel,
          action: "Retry",
          canConnectAnother: true
        };
      case "connected":
        return {
          serverLabel: "local server",
          connectionLabel: "connected",
          refreshLabel,
          action: state.refresh === "degraded" ? "Retry" : null,
          canConnectAnother: false
        };
      case "connecting":
        return {
          serverLabel: "local server",
          connectionLabel: "connecting",
          refreshLabel,
          action: null,
          canConnectAnother: false
        };
    }
  }
  __name(connectionPresentation, "connectionPresentation");
  function markRequestSuccess() {
    snapshot = {
      connection: "connected",
      refresh: snapshot.refresh
    };
    renderConnectionChrome();
  }
  __name(markRequestSuccess, "markRequestSuccess");
  function markRequestFailure(kind) {
    snapshot = kind === "protocol" ? { connection: "connected", refresh: "degraded" } : { ...snapshot, connection: kind };
    renderConnectionChrome();
  }
  __name(markRequestFailure, "markRequestFailure");
  var actions = null;
  function configureConnectionActions(next) {
    actions = next;
  }
  __name(configureConnectionActions, "configureConnectionActions");
  function initConnectionControls() {
    document.querySelector("#connection-action")?.addEventListener("click", () => {
      if (!actions) return;
      if (snapshot.connection === "unauthorized") void actions.reconnect();
      else void actions.retry();
    });
    document.querySelector("#connect-another")?.addEventListener("click", () => {
      if (actions) void actions.reconnect();
    });
    renderConnectionChrome();
  }
  __name(initConnectionControls, "initConnectionControls");
  function renderConnectionChrome() {
    const presentation = connectionPresentation(snapshot);
    const root = document.querySelector("#store-identity");
    root?.classList.remove("hidden");
    const connection = document.querySelector("#connection-status");
    if (connection) connection.textContent = presentation.connectionLabel;
    const refresh = document.querySelector("#refresh-status");
    if (refresh) refresh.textContent = presentation.refreshLabel;
    const legacyRefresh = document.querySelector("#stat-live");
    if (legacyRefresh) {
      legacyRefresh.textContent = presentation.refreshLabel;
      legacyRefresh.dataset.state = snapshot.refresh;
    }
    const dot = document.querySelector("#refresh");
    if (dot) {
      dot.dataset.connection = snapshot.connection;
      dot.dataset.state = snapshot.refresh;
      dot.title = `${presentation.connectionLabel}; refresh ${presentation.refreshLabel}`;
    }
    const action = document.querySelector("#connection-action");
    if (action) {
      action.textContent = presentation.action ?? "";
      action.classList.toggle("hidden", presentation.action === null);
    }
    document.querySelector("#connect-another")?.classList.toggle("hidden", !presentation.canConnectAnother);
    const word = document.querySelector("#refresh-word");
    if (word) {
      word.textContent = snapshot.connection === "unauthorized" ? "authentication required" : snapshot.connection === "unreachable" ? "server unavailable" : snapshot.refresh === "degraded" ? "response error" : "";
    }
  }
  __name(renderConnectionChrome, "renderConnectionChrome");

  // src/change-inspector-http.ts
  var ChangeInspectorRequestFailure = class extends Error {
    constructor(kind, status) {
      super(
        kind === "unauthorized" ? "authentication required" : kind === "unreachable" ? "server unavailable" : "server response error"
      );
      this.kind = kind;
      this.status = status;
    }
    kind;
    status;
    static {
      __name(this, "ChangeInspectorRequestFailure");
    }
  };
  var ChangeInspectorPageFailure = class extends ChangeInspectorRequestFailure {
    constructor(code, status) {
      super("protocol", status);
      this.code = code;
    }
    code;
    static {
      __name(this, "ChangeInspectorPageFailure");
    }
  };
  function failure(kind, status) {
    markRequestFailure(kind);
    return new ChangeInspectorRequestFailure(kind, status);
  }
  __name(failure, "failure");
  function typedPageFailure(value, status) {
    if (typeof value !== "object" || value === null) return null;
    const document2 = value;
    if (document2.schema !== "pointbreak.inspect-change-page-error" || document2.version !== 1)
      return null;
    if (document2.code === "invalid_query" && status === 400)
      return new ChangeInspectorPageFailure("invalid_query", status);
    if (document2.code === "stale_projection" && status === 409)
      return new ChangeInspectorPageFailure("stale_projection", status);
    return null;
  }
  __name(typedPageFailure, "typedPageFailure");
  async function fetchOnce(path) {
    const headers = {};
    const token = getSessionToken();
    if (token) headers.Authorization = `Bearer ${token}`;
    let response;
    try {
      response = await fetch(path, {
        method: "GET",
        cache: "no-store",
        credentials: "omit",
        referrerPolicy: "no-referrer",
        headers
      });
    } catch {
      throw failure("unreachable");
    }
    if (response.status === 401)
      throw new ChangeInspectorRequestFailure("unauthorized", 401);
    let data;
    try {
      data = JSON.parse(await response.text());
    } catch {
      throw failure("protocol", response.status);
    }
    if (!response.ok)
      throw typedPageFailure(data, response.status) ?? failure("protocol", response.status);
    if (typeof data !== "object" || data === null || "error" in data && Boolean(data.error)) {
      throw failure("protocol", response.status);
    }
    markRequestSuccess();
    return data;
  }
  __name(fetchOnce, "fetchOnce");
  async function fetchChangeInspectorJSON(path) {
    const credentialVersion2 = sessionCredentialVersion();
    try {
      return await fetchOnce(path);
    } catch (error) {
      if (!(error instanceof ChangeInspectorRequestFailure) || error.kind !== "unauthorized")
        throw error;
    }
    if (sessionCredentialVersion() !== credentialVersion2 || await recoverUnauthorized())
      return fetchOnce(path);
    throw failure("unauthorized", 401);
  }
  __name(fetchChangeInspectorJSON, "fetchChangeInspectorJSON");

  // src/change-inspector-router.ts
  var QUERY_KEYS = [
    "q",
    "topology",
    "lifecycle",
    "attention",
    "availability",
    "after",
    "limit",
    "order"
  ];
  var ROUTE_QUERY_KEYS = /* @__PURE__ */ new Set([
    ...QUERY_KEYS,
    "artifactHash",
    "fromArtifactHash",
    "toArtifactHash",
    "fact",
    "file"
  ]);
  function decodeSegment(value) {
    try {
      const decoded2 = decodeURIComponent(value);
      return decoded2.length > 0 ? decoded2 : null;
    } catch {
      return null;
    }
  }
  __name(decodeSegment, "decodeSegment");
  function validQueryEncoding(search) {
    if (!search) return true;
    return search.split("&").every((component) => {
      if (!component) return false;
      const separator = component.indexOf("=");
      const key = separator === -1 ? component : component.slice(0, separator);
      const value = separator === -1 ? "" : component.slice(separator + 1);
      try {
        decodeURIComponent(key.replaceAll("+", " "));
        decodeURIComponent(value.replaceAll("+", " "));
        return true;
      } catch {
        return false;
      }
    });
  }
  __name(validQueryEncoding, "validQueryEncoding");
  function parseQuery(search) {
    if (!validQueryEncoding(search)) {
      return { message: "Malformed route query encoding." };
    }
    const params = new URLSearchParams(search);
    const query = {};
    for (const key of params.keys()) {
      if (!ROUTE_QUERY_KEYS.has(key)) {
        return { message: `Unknown ${key} route query.` };
      }
    }
    for (const key of QUERY_KEYS) {
      const values = params.getAll(key);
      if (values.length > 1) return { message: `Duplicate ${key} route query.` };
      const value = values[0];
      if (value === void 0) continue;
      if (key === "limit") {
        const limit = Number(value);
        if (!Number.isInteger(limit))
          return { message: "Invalid limit route query." };
        query.limit = limit;
      } else if (key === "order") {
        if (value !== "change_id_asc") {
          return { message: "Invalid order route query." };
        }
        query.order = value;
      } else {
        query[key] = value;
      }
    }
    return {
      query,
      artifactHashes: params.getAll("artifactHash"),
      fromArtifactHashes: params.getAll("fromArtifactHash"),
      toArtifactHashes: params.getAll("toArtifactHash"),
      facts: params.getAll("fact"),
      files: params.getAll("file")
    };
  }
  __name(parseQuery, "parseQuery");
  function isParseError(value) {
    return "message" in value;
  }
  __name(isParseError, "isParseError");
  function parseChangeInspectorRoute(hash) {
    const raw = hash.startsWith("#") ? hash.slice(1) : hash;
    const separator = raw.indexOf("?");
    const path = separator === -1 ? raw : raw.slice(0, separator);
    const search = separator === -1 ? "" : raw.slice(separator + 1);
    const parsed = parseQuery(search);
    if (isParseError(parsed)) return { kind: "invalid", message: parsed.message };
    const {
      query,
      artifactHashes,
      fromArtifactHashes,
      toArtifactHashes,
      facts,
      files
    } = parsed;
    const focus = /* @__PURE__ */ __name(() => {
      if (facts.length > 1 || files.length > 1 || facts.some((value) => !value) || files.some((value) => !value)) {
        return null;
      }
      const selected = {
        ...facts[0] ? { factId: facts[0] } : {},
        ...files[0] ? { filePath: files[0] } : {}
      };
      return Object.keys(selected).length ? selected : void 0;
    }, "focus");
    const segments = path.split("/").filter(Boolean);
    if (segments.length === 1 && (segments[0] === "changes" || segments[0] === "attention")) {
      if (artifactHashes.length > 0 || fromArtifactHashes.length > 0 || toArtifactHashes.length > 0 || facts.length > 0 || files.length > 0) {
        return {
          kind: "invalid",
          message: "artifactHash is only valid on an exact Revision route."
        };
      }
      return { kind: "lens", lens: segments[0], query };
    }
    if (segments[0] !== "changes")
      return { kind: "invalid", message: "Unknown Change Inspector route." };
    const changeId = decodeSegment(segments[1] ?? "");
    if (changeId === null)
      return { kind: "invalid", message: "Change routes require a Change ID." };
    if (segments.length === 2) {
      if (artifactHashes.length > 0 || fromArtifactHashes.length > 0 || toArtifactHashes.length > 0 || facts.length > 0 || files.length > 0) {
        return {
          kind: "invalid",
          message: "artifactHash is only valid on an exact Revision route."
        };
      }
      return { kind: "change", changeId, query };
    }
    const exactRevision = /* @__PURE__ */ __name((revisionId) => {
      if (revisionId === null || artifactHashes.length !== 1 || !artifactHashes[0])
        return null;
      return {
        revisionId,
        objectArtifactContentHash: artifactHashes[0]
      };
    }, "exactRevision");
    const exactFailure = /* @__PURE__ */ __name(() => ({
      kind: "invalid",
      message: artifactHashes.length > 1 ? "Exact Revision routes require exactly one artifactHash." : "Exact Revision routes require artifactHash."
    }), "exactFailure");
    if (segments[2] === "revisions" && segments.length >= 4) {
      const revision = exactRevision(decodeSegment(segments[3]));
      if (revision === null) return exactFailure();
      const exactFocus = focus();
      if (exactFocus === null)
        return {
          kind: "invalid",
          message: "Exact route focus requires at most one non-empty fact and file."
        };
      if (fromArtifactHashes.length > 0 || toArtifactHashes.length > 0)
        return {
          kind: "invalid",
          message: "Revision routes do not accept interdiff hashes."
        };
      if (segments.length === 4)
        return {
          kind: "revision",
          changeId,
          revision,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
      if (segments.length === 5 && segments[4] === "resource")
        return {
          kind: "resource",
          changeId,
          revision,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
      if (segments.length === 5 && segments[4] === "association")
        return {
          kind: "association",
          changeId,
          revision,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
    }
    if (segments[2] === "interdiff" && segments.length === 5) {
      if (artifactHashes.length > 0)
        return {
          kind: "invalid",
          message: "Interdiff routes use endpoint artifact hashes."
        };
      const fromRevisionId = decodeSegment(segments[3]);
      const toRevisionId = decodeSegment(segments[4]);
      if (fromRevisionId === null || toRevisionId === null)
        return {
          kind: "invalid",
          message: "Interdiff routes require both Revision IDs."
        };
      if (fromArtifactHashes.length !== 1 || !fromArtifactHashes[0] || toArtifactHashes.length !== 1 || !toArtifactHashes[0])
        return {
          kind: "invalid",
          message: "Interdiff routes require exactly one artifact hash for each endpoint."
        };
      const exactFocus = focus();
      if (exactFocus === null)
        return {
          kind: "invalid",
          message: "Exact route focus requires at most one non-empty fact and file."
        };
      return {
        kind: "interdiff",
        changeId,
        from: {
          revisionId: fromRevisionId,
          objectArtifactContentHash: fromArtifactHashes[0]
        },
        to: {
          revisionId: toRevisionId,
          objectArtifactContentHash: toArtifactHashes[0]
        },
        query,
        ...exactFocus ? { focus: exactFocus } : {}
      };
    }
    return { kind: "invalid", message: "Unknown Change Inspector route." };
  }
  __name(parseChangeInspectorRoute, "parseChangeInspectorRoute");
  function appendQuery(query, params) {
    for (const key of QUERY_KEYS) {
      const value = query[key];
      if (value !== void 0) params.set(key, String(value));
    }
  }
  __name(appendQuery, "appendQuery");
  function formatChangeInspectorRoute(route) {
    const params = new URLSearchParams();
    appendQuery(route.query, params);
    if (route.kind === "revision" || route.kind === "resource" || route.kind === "association")
      params.set("artifactHash", route.revision.objectArtifactContentHash);
    if (route.kind === "interdiff") {
      params.set("fromArtifactHash", route.from.objectArtifactContentHash);
      params.set("toArtifactHash", route.to.objectArtifactContentHash);
    }
    if ("focus" in route && route.focus?.factId)
      params.set("fact", route.focus.factId);
    if ("focus" in route && route.focus?.filePath)
      params.set("file", route.focus.filePath);
    const suffix = params.size ? `?${params}` : "";
    if (route.kind === "lens") return `#/${route.lens}${suffix}`;
    const change = encodeURIComponent(route.changeId);
    if (route.kind === "change") return `#/changes/${change}${suffix}`;
    if (route.kind === "revision")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}${suffix}`;
    if (route.kind === "resource")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/resource${suffix}`;
    if (route.kind === "association")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/association${suffix}`;
    return `#/changes/${change}/interdiff/${encodeURIComponent(route.from.revisionId)}/${encodeURIComponent(route.to.revisionId)}${suffix}`;
  }
  __name(formatChangeInspectorRoute, "formatChangeInspectorRoute");
  function lensForRoute(route) {
    return route.kind === "lens" ? route.lens : "changes";
  }
  __name(lensForRoute, "lensForRoute");
  function firstPageQuery(query) {
    const { after: _after, ...firstPage } = query;
    return firstPage;
  }
  __name(firstPageQuery, "firstPageQuery");
  function queryForExactNavigation(route) {
    if (route.kind !== "lens" || route.lens !== "attention") return route.query;
    return firstPageQuery(route.query);
  }
  __name(queryForExactNavigation, "queryForExactNavigation");

  // src/dom.ts
  function $(sel) {
    return document.querySelector(sel);
  }
  __name($, "$");

  // src/prefs.ts
  var THEME_KEY = "shore-inspect-theme";
  var DENSITY_KEY = "shore-inspect-density";
  var SPLIT_KEY = "shore-inspect-split";
  var SPLIT_MIN = 25;
  var SPLIT_MAX = 75;
  var liveMediaQueries = [];
  function preferredThemeMode() {
    const stored = localStorage.getItem(THEME_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  }
  __name(preferredThemeMode, "preferredThemeMode");
  function hasPinnedTheme() {
    return preferredThemeMode() !== "system";
  }
  __name(hasPinnedTheme, "hasPinnedTheme");
  function osTheme() {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  __name(osTheme, "osTheme");
  function preferredTheme() {
    const mode = preferredThemeMode();
    return mode === "system" ? osTheme() : mode;
  }
  __name(preferredTheme, "preferredTheme");
  function syncChoice(name, value) {
    for (const input of document.querySelectorAll(
      `input[name="${name}"]`
    )) {
      input.checked = input.value === value;
    }
  }
  __name(syncChoice, "syncChoice");
  function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme);
    syncChoice("theme-mode", preferredThemeMode());
  }
  __name(applyTheme, "applyTheme");
  function setThemeMode(mode) {
    const next = mode === "light" || mode === "dark" ? mode : "system";
    localStorage.setItem(THEME_KEY, next);
    applyTheme(preferredTheme());
  }
  __name(setThemeMode, "setThemeMode");
  function preferredDensity() {
    return localStorage.getItem(DENSITY_KEY) || "comfortable";
  }
  __name(preferredDensity, "preferredDensity");
  function applyDensity(mode) {
    const value = mode === "compact" ? "compact" : "comfortable";
    document.documentElement.classList.toggle("compact", value === "compact");
    syncChoice("density-mode", value);
  }
  __name(applyDensity, "applyDensity");
  function setDensity(mode) {
    const next = mode === "compact" ? "compact" : "comfortable";
    localStorage.setItem(DENSITY_KEY, next);
    applyDensity(next);
  }
  __name(setDensity, "setDensity");
  function preferredSplit() {
    const raw = localStorage.getItem(SPLIT_KEY);
    const n = raw === null ? Number.NaN : Number.parseInt(raw, 10);
    return Number.isInteger(n) && n >= SPLIT_MIN && n <= SPLIT_MAX ? n : null;
  }
  __name(preferredSplit, "preferredSplit");
  function applySplit(pct) {
    if (pct === null) {
      document.documentElement.style.removeProperty("--split-master");
      localStorage.removeItem(SPLIT_KEY);
      return;
    }
    const clamped = Math.round(Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, pct)));
    document.documentElement.style.setProperty("--split-master", `${clamped}%`);
    localStorage.setItem(SPLIT_KEY, String(clamped));
  }
  __name(applySplit, "applySplit");
  function applyPrefs() {
    applyTheme(preferredTheme());
    applyDensity(preferredDensity());
    const split = preferredSplit();
    if (split !== null) applySplit(split);
  }
  __name(applyPrefs, "applyPrefs");
  function watchColorScheme() {
    const query = window.matchMedia("(prefers-color-scheme: light)");
    liveMediaQueries.push(query);
    query.addEventListener("change", () => {
      if (hasPinnedTheme()) return;
      applyTheme(preferredTheme());
    });
  }
  __name(watchColorScheme, "watchColorScheme");

  // src/change-inspector-interaction.ts
  var colorSchemeWatcherInstalled = false;
  var HISTORY_ORIGIN_KEY = "__pointbreakChangeInspectorOrigin";
  function isTextControl(target) {
    if (!(target instanceof HTMLElement)) return false;
    return target.isContentEditable || target.matches(
      "input, textarea, select, [role='textbox'], [role='combobox']"
    );
  }
  __name(isTextControl, "isTextControl");
  function isNativeActionControl(target) {
    return target instanceof Element && target.closest(
      "button, a[href], [role='button'], [role='link'], [role='separator']"
    ) !== null;
  }
  __name(isNativeActionControl, "isNativeActionControl");
  function setSelected(changeId) {
    document.querySelectorAll(".unit-card[data-change-id]").forEach((card) => {
      const selected = card.dataset.changeId === changeId;
      card.classList.toggle("change-card-selected", selected);
      card.setAttribute("aria-current", selected ? "true" : "false");
    });
  }
  __name(setSelected, "setSelected");
  function moveSelection(selectedChangeId, delta) {
    const cards = Array.from(
      document.querySelectorAll(".unit-card[data-change-id]")
    );
    if (!cards.length) return selectedChangeId;
    const current = cards.findIndex(
      (card2) => card2.dataset.changeId === selectedChangeId
    );
    const next = Math.max(
      0,
      Math.min(cards.length - 1, current < 0 ? 0 : current + delta)
    );
    const card = cards[next];
    const changeId = card.dataset.changeId ?? null;
    setSelected(changeId);
    card.scrollIntoView({ block: "nearest", behavior: "auto" });
    return changeId;
  }
  __name(moveSelection, "moveSelection");
  function moveSelectionToBoundary(selectedChangeId, boundary) {
    const cards = Array.from(
      document.querySelectorAll(".unit-card[data-change-id]")
    );
    const card = boundary === "first" ? cards[0] : cards.at(-1);
    if (!card) return selectedChangeId;
    const changeId = card.dataset.changeId ?? null;
    setSelected(changeId);
    card.scrollIntoView({ block: "nearest", behavior: "auto" });
    return changeId;
  }
  __name(moveSelectionToBoundary, "moveSelectionToBoundary");
  function trapModalFocus(modal, event) {
    if (event.key !== "Tab") return;
    const stops = Array.from(
      modal.querySelectorAll(
        "button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])"
      )
    );
    if (!stops.length) return;
    const active2 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first = stops[0];
    const last = stops.at(-1) ?? first;
    if (event.shiftKey && (active2 === first || !modal.contains(active2))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active2 === last || !modal.contains(active2))) {
      event.preventDefault();
      first.focus();
    }
  }
  __name(trapModalFocus, "trapModalFocus");
  function installChangeInspectorInteraction(actions2) {
    let selectedChangeId = null;
    let modalReturnFocus = null;
    let detailReturnFocus = null;
    let detailWasOpen = false;
    let currentRoute2 = null;
    let exactOriginLens = null;
    let detailDomIdentity = null;
    applyPrefs();
    if (!colorSchemeWatcherInstalled) {
      watchColorScheme();
      colorSchemeWatcherInstalled = true;
    }
    const historyOrigin = /* @__PURE__ */ __name((route) => {
      if (route.kind === "lens") return null;
      const state = history.state;
      if (state === null || typeof state !== "object" || Array.isArray(state))
        return null;
      const origin = state[HISTORY_ORIGIN_KEY];
      if (origin === null || typeof origin !== "object") return null;
      const record = origin;
      if (record.route !== formatChangeInspectorRoute(route)) return null;
      return record.lens === "changes" || record.lens === "attention" ? record.lens : null;
    }, "historyOrigin");
    const persistHistoryOrigin = /* @__PURE__ */ __name((route, lens) => {
      if (route.kind === "lens") return;
      const state = history.state;
      const retained = state !== null && typeof state === "object" && !Array.isArray(state) ? state : {};
      history.replaceState(
        {
          ...retained,
          [HISTORY_ORIGIN_KEY]: {
            route: formatChangeInspectorRoute(route),
            lens
          }
        },
        "",
        location.href
      );
    }, "persistHistoryOrigin");
    const listRoute = /* @__PURE__ */ __name((route) => ({
      kind: "lens",
      lens: route.kind === "lens" ? route.lens : historyOrigin(route) ?? exactOriginLens ?? "changes",
      query: route.query
    }), "listRoute");
    const focusFallback = /* @__PURE__ */ __name((route = currentRoute2) => {
      const target = route !== null && route.kind !== "lens" ? window.matchMedia("(max-width: 760px)").matches ? document.querySelector("#detail-back") : document.querySelector("#detail-close") : document.querySelector("#master");
      target?.focus({ preventScroll: true });
    }, "focusFallback");
    const closeModal = /* @__PURE__ */ __name((id) => {
      const modal = document.querySelector(id);
      if (!modal || modal.classList.contains("hidden")) return;
      modal.classList.add("hidden");
      const focus = modalReturnFocus;
      modalReturnFocus = null;
      if (focus?.isConnected === true) focus.focus({ preventScroll: true });
      else focusFallback();
    }, "closeModal");
    const openModal = /* @__PURE__ */ __name((id, initial) => {
      const modal = document.querySelector(id);
      if (!modal) return;
      modalReturnFocus = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
      modal.classList.remove("hidden");
      initial?.focus();
    }, "openModal");
    const changeTheme = /* @__PURE__ */ __name((event) => {
      const input = event.target;
      if (!input.checked) return;
      setThemeMode(input.value);
    }, "changeTheme");
    const changeDensity = /* @__PURE__ */ __name((event) => {
      const input = event.target;
      if (!input.checked) return;
      setDensity(input.value);
    }, "changeDensity");
    document.querySelectorAll("input[name='theme-mode']").forEach((input) => {
      input.addEventListener("change", changeTheme);
    });
    document.querySelectorAll("input[name='density-mode']").forEach((input) => {
      input.addEventListener("change", changeDensity);
    });
    const paletteInput = document.querySelector("#cmd-input");
    const paletteResults = document.querySelector("#cmd-results");
    const paletteCommands = [
      ["Open Changes", "changes"],
      ["Open Attention", "attention"]
    ];
    const renderPaletteResults = /* @__PURE__ */ __name(() => {
      if (paletteResults) {
        paletteResults.replaceChildren();
        const query = paletteInput?.value.trim().toLocaleLowerCase() ?? "";
        const matching = paletteCommands.filter(
          ([label]) => label.toLocaleLowerCase().includes(query)
        );
        for (const [label, lens] of matching) {
          const button = document.createElement("button");
          button.type = "button";
          button.className = "ghost cmd-item";
          const commandLabel = document.createElement("span");
          commandLabel.className = "cmd-label";
          commandLabel.textContent = label;
          button.append(commandLabel);
          button.addEventListener("click", () => {
            closeModal("#cmd-palette");
            const route = currentRoute2;
            if (route)
              actions2.navigate({
                kind: "lens",
                lens,
                query: { ...route.query, after: void 0 }
              });
          });
          paletteResults.append(button);
        }
        if (matching.length === 0) {
          const empty = document.createElement("p");
          empty.className = "cmd-empty";
          empty.setAttribute("role", "status");
          empty.textContent = "No matching commands.";
          paletteResults.append(empty);
        }
      }
    }, "renderPaletteResults");
    const openPalette = /* @__PURE__ */ __name(() => {
      if (paletteInput) paletteInput.value = "";
      renderPaletteResults();
      openModal("#cmd-palette", paletteInput);
    }, "openPalette");
    paletteInput?.addEventListener("input", renderPaletteResults);
    const helpClose = /* @__PURE__ */ __name(() => closeModal("#key-help"), "helpClose");
    const helpCloseButton = document.querySelector("#key-help-close");
    helpCloseButton?.addEventListener("click", helpClose);
    const readingButton = document.querySelector("#detail-read");
    const masterRail = document.querySelector("#master-rail");
    const setReading = /* @__PURE__ */ __name((enabled) => {
      const split = document.querySelector(".split");
      const detail = document.querySelector("#detail");
      const scrollTop = detail?.scrollTop ?? 0;
      split?.classList.toggle("reading", enabled);
      if (readingButton) {
        readingButton.textContent = enabled ? "⤡" : "⤢";
        readingButton.setAttribute("aria-pressed", String(enabled));
        readingButton.setAttribute(
          "aria-label",
          enabled ? "Exit reading mode" : "Enter reading mode"
        );
        readingButton.title = enabled ? "Exit reading mode" : "Reading mode";
      }
      if (detail) detail.scrollTop = scrollTop;
    }, "setReading");
    const toggleReading = /* @__PURE__ */ __name(() => {
      setReading(
        !document.querySelector(".split")?.classList.contains("reading")
      );
    }, "toggleReading");
    readingButton?.addEventListener("click", toggleReading);
    const restoreMaster = /* @__PURE__ */ __name(() => {
      setReading(false);
      document.querySelector("#master")?.focus({ preventScroll: true });
    }, "restoreMaster");
    masterRail?.addEventListener("click", restoreMaster);
    const divider = document.querySelector(".divider");
    const updateSplit = /* @__PURE__ */ __name((value) => {
      applySplit(value);
      divider?.setAttribute("aria-valuenow", String(preferredSplit() ?? 50));
    }, "updateSplit");
    updateSplit(preferredSplit());
    const onDividerKey = /* @__PURE__ */ __name((event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        updateSplit(null);
        return;
      }
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      event.stopPropagation();
      const value = (preferredSplit() ?? 50) + (event.key === "ArrowLeft" ? -5 : 5);
      updateSplit(value);
    }, "onDividerKey");
    divider?.addEventListener("keydown", onDividerKey);
    let activeDividerPointerId = null;
    const finishDividerDrag = /* @__PURE__ */ __name((event) => {
      if (!divider || activeDividerPointerId !== event.pointerId) return;
      activeDividerPointerId = null;
      divider.classList.remove("dragging");
      if (divider.hasPointerCapture?.(event.pointerId)) {
        divider.releasePointerCapture?.(event.pointerId);
      }
    }, "finishDividerDrag");
    const onDividerLostPointerCapture = /* @__PURE__ */ __name((event) => {
      if (!divider || activeDividerPointerId !== event.pointerId) return;
      activeDividerPointerId = null;
      divider.classList.remove("dragging");
    }, "onDividerLostPointerCapture");
    const onDividerPointerDown = /* @__PURE__ */ __name((event) => {
      if (!divider || activeDividerPointerId !== null || event.pointerType === "mouse" && event.button !== 0)
        return;
      event.preventDefault();
      divider.focus();
      activeDividerPointerId = event.pointerId;
      divider.setPointerCapture?.(event.pointerId);
      divider.classList.add("dragging");
    }, "onDividerPointerDown");
    const onDividerPointerMove = /* @__PURE__ */ __name((event) => {
      if (!divider?.classList.contains("dragging") || activeDividerPointerId !== event.pointerId)
        return;
      const split = document.querySelector(".split");
      const bounds = split?.getBoundingClientRect();
      if (!bounds || bounds.width <= 0) return;
      const value = (event.clientX - bounds.left) / bounds.width * 100;
      if (value < 15) {
        finishDividerDrag(event);
        setReading(true);
        return;
      }
      updateSplit(value);
    }, "onDividerPointerMove");
    const onDividerDoubleClick = /* @__PURE__ */ __name((event) => {
      event.preventDefault();
      updateSplit(null);
    }, "onDividerDoubleClick");
    divider?.addEventListener("pointerdown", onDividerPointerDown);
    divider?.addEventListener("pointermove", onDividerPointerMove);
    divider?.addEventListener("pointerup", finishDividerDrag);
    divider?.addEventListener("pointercancel", finishDividerDrag);
    divider?.addEventListener("lostpointercapture", onDividerLostPointerCapture);
    divider?.addEventListener("dblclick", onDividerDoubleClick);
    const onClick = /* @__PURE__ */ __name((event) => {
      const target = event.target instanceof Element ? event.target : null;
      const card = target?.closest(".unit-card[data-change-id]");
      if (card && !target?.closest("button, a, input, select, textarea")) {
        selectedChangeId = card.dataset.changeId ?? null;
        setSelected(selectedChangeId);
      }
    }, "onClick");
    document.addEventListener("click", onClick);
    const onKey = /* @__PURE__ */ __name((event) => {
      const open = document.querySelector(
        "#cmd-palette:not(.hidden), #key-help:not(.hidden)"
      );
      if (open) {
        if (event.key === "Escape") {
          event.preventDefault();
          closeModal(`#${open.id}`);
        } else {
          trapModalFocus(open, event);
        }
        return;
      }
      if (document.querySelector("#reconnect-dialog:not(.hidden)")) return;
      if (isTextControl(event.target)) return;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k" || event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        openPalette();
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const route = currentRoute2;
      if (!route) return;
      if (event.key === "?") {
        event.preventDefault();
        openModal("#key-help", document.querySelector("#key-help-close"));
        return;
      }
      if (event.key === "/") {
        event.preventDefault();
        document.querySelector("#filter-text")?.focus();
        return;
      }
      if (event.key === "j" || event.key === "ArrowDown") {
        event.preventDefault();
        selectedChangeId = moveSelection(selectedChangeId, 1);
        return;
      }
      if (event.key === "k" || event.key === "ArrowUp") {
        event.preventDefault();
        selectedChangeId = moveSelection(selectedChangeId, -1);
        return;
      }
      if (event.key === "g") {
        event.preventDefault();
        selectedChangeId = moveSelectionToBoundary(selectedChangeId, "first");
        return;
      }
      if (event.key === "G") {
        event.preventDefault();
        selectedChangeId = moveSelectionToBoundary(selectedChangeId, "last");
        return;
      }
      if (event.key === "1") {
        event.preventDefault();
        actions2.navigate({
          kind: "lens",
          lens: "changes",
          query: { ...route.query, after: void 0 }
        });
        return;
      }
      if (event.key === "2") {
        event.preventDefault();
        actions2.navigate({
          kind: "lens",
          lens: "attention",
          query: { ...route.query, after: void 0 }
        });
        return;
      }
      if (event.key === "Enter" && selectedChangeId && !isNativeActionControl(event.target)) {
        event.preventDefault();
        actions2.navigate({
          kind: "change",
          changeId: selectedChangeId,
          query: queryForExactNavigation(route)
        });
        return;
      }
      if (event.key === "h") {
        event.preventDefault();
        updateSplit((preferredSplit() ?? 50) - 5);
        return;
      }
      if (event.key === "l") {
        event.preventDefault();
        updateSplit((preferredSplit() ?? 50) + 5);
        return;
      }
      if (event.key === "Escape" && route.kind !== "lens") {
        event.preventDefault();
        actions2.navigate(listRoute(route));
      }
    }, "onKey");
    document.addEventListener("keydown", onKey);
    const onClose = /* @__PURE__ */ __name(() => {
      const route = currentRoute2;
      if (route) actions2.navigate(listRoute(route));
    }, "onClose");
    const closeButton = document.querySelector("#detail-close");
    const backButton = document.querySelector("#detail-back");
    if (closeButton) closeButton.onclick = onClose;
    if (backButton) backButton.onclick = onClose;
    const stop = /* @__PURE__ */ __name(() => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
      document.querySelectorAll("input[name='theme-mode']").forEach((input) => {
        input.removeEventListener("change", changeTheme);
      });
      document.querySelectorAll("input[name='density-mode']").forEach((input) => {
        input.removeEventListener("change", changeDensity);
      });
      helpCloseButton?.removeEventListener("click", helpClose);
      readingButton?.removeEventListener("click", toggleReading);
      masterRail?.removeEventListener("click", restoreMaster);
      divider?.removeEventListener("keydown", onDividerKey);
      divider?.removeEventListener("pointerdown", onDividerPointerDown);
      divider?.removeEventListener("pointermove", onDividerPointerMove);
      divider?.removeEventListener("pointerup", finishDividerDrag);
      divider?.removeEventListener("pointercancel", finishDividerDrag);
      divider?.removeEventListener(
        "lostpointercapture",
        onDividerLostPointerCapture
      );
      divider?.removeEventListener("dblclick", onDividerDoubleClick);
      if (divider && activeDividerPointerId !== null && divider.hasPointerCapture?.(activeDividerPointerId)) {
        divider.releasePointerCapture?.(activeDividerPointerId);
      }
      activeDividerPointerId = null;
      divider?.classList.remove("dragging");
      paletteInput?.removeEventListener("input", renderPaletteResults);
      if (closeButton?.onclick === onClose) closeButton.onclick = null;
      if (backButton?.onclick === onClose) backButton.onclick = null;
      document.querySelector("#cmd-palette")?.classList.add("hidden");
      document.querySelector("#key-help")?.classList.add("hidden");
      paletteResults?.replaceChildren();
      selectedChangeId = null;
      setSelected(null);
      modalReturnFocus = null;
      detailReturnFocus = null;
      detailWasOpen = false;
      currentRoute2 = null;
      exactOriginLens = null;
      detailDomIdentity = null;
    }, "stop");
    return {
      sync(snapshot2) {
        const nextRoute = snapshot2.route.kind === "invalid" ? null : snapshot2.route;
        if (nextRoute !== null && nextRoute.kind !== "lens") {
          const persistedOrigin = historyOrigin(nextRoute);
          const origin = persistedOrigin ?? (currentRoute2?.kind === "lens" ? currentRoute2.lens : exactOriginLens ?? "changes");
          exactOriginLens = origin;
          if (persistedOrigin === null) persistHistoryOrigin(nextRoute, origin);
        } else {
          exactOriginLens = null;
        }
        const cards = Array.from(
          document.querySelectorAll(".unit-card[data-change-id]")
        );
        if (!cards.some((card) => card.dataset.changeId === selectedChangeId))
          selectedChangeId = null;
        setSelected(selectedChangeId);
        const detailOpen = snapshot2.route.kind !== "lens" && snapshot2.route.kind !== "invalid";
        const detail = document.querySelector("#detail");
        const nextDetailDomIdentity = document.querySelector("#detail-body")?.firstChild ?? null;
        const detailDomChanged = detailDomIdentity !== nextDetailDomIdentity;
        document.querySelector(".split")?.classList.toggle("split-closed", !detailOpen);
        if (detail) {
          detail.inert = !detailOpen;
          if (detailOpen) detail.removeAttribute("aria-hidden");
          else detail.setAttribute("aria-hidden", "true");
        }
        if (detailOpen && !detailWasOpen) {
          const active2 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          detailReturnFocus = active2 && active2 !== document.body ? active2 : null;
          if (window.matchMedia("(max-width: 760px)").matches) {
            document.querySelector("#detail-back")?.focus({ preventScroll: true });
          }
        } else if (detailOpen && detailWasOpen && detailDomChanged && (!(document.activeElement instanceof HTMLElement) || document.activeElement === document.body || !document.activeElement.isConnected)) {
          focusFallback(nextRoute);
        } else if (!detailOpen && detailWasOpen) {
          setReading(false);
          const candidate = detailReturnFocus?.isConnected === true ? detailReturnFocus : document.querySelector("#master");
          detailReturnFocus = null;
          candidate?.focus({ preventScroll: true });
        }
        detailWasOpen = detailOpen;
        currentRoute2 = nextRoute;
        detailDomIdentity = nextDetailDomIdentity;
      },
      stop
    };
  }
  __name(installChangeInspectorInteraction, "installChangeInspectorInteraction");

  // ../../../documents/change_reader_profile_v1.json
  var change_reader_profile_v1_default = {
    minimumReaderProfile: "review_change_revision_v1",
    documents: {
      "pointbreak.attention-list": 2,
      "pointbreak.inspect-attention": 2,
      "pointbreak.inspect-changes-page": 1,
      "pointbreak.inspect-reader-profile": 1,
      "pointbreak.reader-upgrade-required": 1,
      "pointbreak.review-association-comparison": 1,
      "pointbreak.review-change": 1,
      "pointbreak.review-change-list": 1,
      "pointbreak.review-change-revision": 1,
      "pointbreak.review-revision": 3,
      "pointbreak.review-revision-interdiff": 1,
      "pointbreak.review-revision-resource": 1,
      "pointbreak.store-migration-in-progress": 1,
      "pointbreak.store-migration-required": 1
    }
  };

  // src/change-protocol.ts
  var CHANGE_PAGE_LIMIT = 50;
  var CHANGE_READER_PROFILE = change_reader_profile_v1_default.minimumReaderProfile;
  var CHANGE_READER_DOCUMENTS = change_reader_profile_v1_default.documents;
  var TOPOLOGY_VALUES = /* @__PURE__ */ new Set([
    "initial",
    "replacement",
    "replacement_divergent",
    "consolidation",
    "parallel_current",
    "mixed",
    "incomplete",
    "cycle_conflicted"
  ]);
  var LIFECYCLE_VALUES = /* @__PURE__ */ new Set([
    "incomplete",
    "conflicted",
    "in_progress",
    "accepted"
  ]);
  var ATTENTION_VALUES = /* @__PURE__ */ new Set([
    "clear",
    "in_progress",
    "incomplete",
    "conflicted"
  ]);
  var AVAILABILITY_VALUES = /* @__PURE__ */ new Set(["available", "incomplete"]);
  var CONTENT_AVAILABILITY_VALUES = /* @__PURE__ */ new Set([
    "available",
    "removed",
    "missing",
    "mismatch",
    "non_textual"
  ]);
  var REVISION_CURRENCY_VALUES = /* @__PURE__ */ new Set([
    "current",
    "stale_by_supersession",
    "membership_incomplete",
    "membership_conflicted"
  ]);
  var FACT_FAMILY_STATE_VALUES = /* @__PURE__ */ new Set([
    "current",
    "stale",
    "withdrawn",
    "conflicted",
    "unavailable"
  ]);
  var ASSOCIATION_STATE_VALUES = /* @__PURE__ */ new Set([
    "unknown",
    "exact",
    "equivalent",
    "extension",
    "unavailable"
  ]);
  var ASSOCIATION_PROOF_VALUES = /* @__PURE__ */ new Set([
    "available",
    "missing",
    "mismatch",
    "not_requested"
  ]);
  var INTERDIFF_AVAILABILITY_VALUES = /* @__PURE__ */ new Set([
    "available",
    "unavailable",
    "endpoint_missing",
    "endpoint_mismatch",
    "non_textual"
  ]);
  function decodeChangeDetail(value) {
    const detail = object(value, "Change detail");
    const summary = detail.summary;
    const stamp = detail.projectionStamp;
    const memberRevisions = detail.memberRevisions;
    const unavailableMemberRevisions = detail.unavailableMemberRevisions;
    const membershipClaims = detail.membershipClaims;
    const membershipWithdrawals = detail.membershipWithdrawals;
    const relationClaims = detail.relationClaims;
    const relationWithdrawals = detail.relationWithdrawals;
    const links = detail.links;
    const effectiveSupersedes = detail.effectiveSupersedes;
    const pendingOrConflictingEdges = detail.pendingOrConflictingEdges;
    const currentRevisionRefs = detail.currentRevisionRefs;
    const perCurrentRevisionQualification = detail.perCurrentRevisionQualification;
    const operativeObligations = detail.operativeObligations;
    const diagnostics = detail.diagnostics;
    if (detail.schema !== "pointbreak.review-change" || detail.version !== 1 || !nonEmptyString(stamp) || !isChangeSummary(summary, stamp) || !isChangeMemberRevisions(memberRevisions) || !isUnavailableChangeMemberRevisions(unavailableMemberRevisions) || !isMembershipClaims(membershipClaims, summary.changeId) || !isClaimWithdrawals(membershipWithdrawals) || !Array.isArray(relationClaims) || !relationClaims.every(
      (claim) => isRelationClaim(claim, summary.changeId)
    ) || !isClaimWithdrawals(relationWithdrawals) || !isChangeLinks(links) || !isEffectiveSupersedes(effectiveSupersedes) || !Array.isArray(pendingOrConflictingEdges) || !pendingOrConflictingEdges.every(
      (claim) => isRelationClaim(claim, summary.changeId)
    ) || !Array.isArray(currentRevisionRefs) || !currentRevisionRefs.every(isRevisionRef) || !sameRevisionSet(currentRevisionRefs, summary.currentRevisionRefs) || !isRevisionQualifications(
      perCurrentRevisionQualification,
      currentRevisionRefs
    ) || !isStringArray(operativeObligations) || !isStringArray(diagnostics)) {
      throw new Error("invalid Change detail DTO");
    }
    return {
      schema: "pointbreak.review-change",
      version: 1,
      summary,
      memberRevisions,
      unavailableMemberRevisions,
      membershipClaims,
      membershipWithdrawals,
      relationClaims,
      relationWithdrawals,
      links,
      effectiveSupersedes,
      pendingOrConflictingEdges,
      currentRevisionRefs,
      perCurrentRevisionQualification,
      operativeObligations,
      diagnostics,
      projectionStamp: stamp
    };
  }
  __name(decodeChangeDetail, "decodeChangeDetail");
  function decodeChangeRevisionDetail(value) {
    const detail = object(value, "Change Revision detail");
    const revision = detail.revision;
    const factPresentations = detail.factPresentations;
    const factContentPresentations = detail.factContentPresentations;
    const exactRevisionDocument = detail.exactRevisionDocument;
    const membershipSupport = detail.membershipSupport;
    const factPorts = detail.factPorts;
    const associations = detail.associations;
    const diagnostics = detail.diagnostics;
    const revisionCurrency = detail.revisionCurrency;
    const relationClassification = detail.relationClassification;
    const availability = detail.availability;
    if (detail.schema !== "pointbreak.review-change-revision" || detail.version !== 1 || !nonEmptyString(detail.changeId) || !isRevisionRef(revision) || typeof revisionCurrency !== "string" || !REVISION_CURRENCY_VALUES.has(revisionCurrency) || relationClassification !== "current" && relationClassification !== "superseded" || typeof availability !== "string" || !CONTENT_AVAILABILITY_VALUES.has(availability) || !isRevisionResource(exactRevisionDocument) || !sameRevision(exactRevisionDocument.resource.revision, revision) || availability !== exactRevisionDocument.availability || !isMembershipClaims(membershipSupport, detail.changeId) || !Array.isArray(factPresentations) || !factPresentations.every(isFactPresentation) || !uniqueFactPresentationIds(factPresentations) || factContentPresentations !== void 0 && !isFactContentPresentations(factContentPresentations) || factContentPresentations !== void 0 && !sameFactIds(factPresentations, factContentPresentations) || !isFactPortPresentations(
      factPorts,
      detail.changeId,
      factPresentations,
      revision
    ) || !Array.isArray(associations) || !associations.every(isAssociation) || !isStringArray(diagnostics) || !nonEmptyString(detail.projectionStamp)) {
      throw new Error("invalid Change Revision detail DTO");
    }
    return {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: detail.changeId,
      revision,
      membershipSupport,
      revisionCurrency,
      relationClassification,
      availability,
      exactRevisionDocument,
      factPresentations,
      factContentPresentations,
      factPorts,
      associations,
      diagnostics,
      projectionStamp: detail.projectionStamp
    };
  }
  __name(decodeChangeRevisionDetail, "decodeChangeRevisionDetail");
  function decodeRevisionResource(value) {
    const document2 = object(value, "Revision resource");
    const resource = document2.resource;
    const projection = document2.projection;
    const diagnostics = document2.diagnostics;
    const availability = document2.availability;
    const capturedDocumentHash = document2.capturedDocumentHash;
    const projectionStamp = document2.projectionStamp;
    const cacheKey = document2.cacheKey;
    if (document2.schema !== "pointbreak.review-revision-resource" || document2.version !== 1 || !isRecord(resource) || !isRevisionRef(resource.revision) || !nonEmptyString(resource.objectId) || !isResourceProjection(projection) || !isOneOf(availability, CONTENT_AVAILABILITY_VALUES) || capturedDocumentHash !== void 0 && !nonEmptyString(capturedDocumentHash) || availability === "available" && (capturedDocumentHash === void 0 || !isCapturedReviewSnapshot(
      document2.capturedDocument,
      resource.revision.objectArtifactContentHash,
      resource.objectId
    )) || availability !== "available" && (capturedDocumentHash !== void 0 || document2.capturedDocument !== void 0) || !nonEmptyString(projectionStamp) || !nonEmptyString(cacheKey) || !isStringArray(diagnostics)) {
      throw new Error("invalid Revision resource DTO");
    }
    return {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      resource: { revision: resource.revision, objectId: resource.objectId },
      projection,
      availability,
      capturedDocumentHash,
      capturedDocument: document2.capturedDocument,
      diagnostics,
      projectionStamp,
      cacheKey
    };
  }
  __name(decodeRevisionResource, "decodeRevisionResource");
  function decodeRevisionInterdiff(value) {
    const document2 = object(value, "Revision interdiff");
    const interdiff = document2.interdiff;
    const diagnostics = document2.diagnostics;
    const availability = document2.availability;
    const projectionStamp = document2.projectionStamp;
    const cacheKey = document2.cacheKey;
    if (document2.schema !== "pointbreak.review-revision-interdiff" || document2.version !== 1 || !isRecord(interdiff) || !isRevisionRef(interdiff.from) || !isRevisionRef(interdiff.to) || !nonEmptyString(interdiff.algorithmVersion) || !isStringArray(interdiff.scope) || !isOneOf(availability, INTERDIFF_AVAILABILITY_VALUES) || !isStringArray(diagnostics) || !nonEmptyString(projectionStamp) || !nonEmptyString(cacheKey) || availability === "available" !== (document2.comparison !== void 0)) {
      throw new Error("invalid Revision interdiff DTO");
    }
    return {
      schema: "pointbreak.review-revision-interdiff",
      version: 1,
      interdiff: {
        from: interdiff.from,
        to: interdiff.to,
        algorithmVersion: interdiff.algorithmVersion,
        scope: interdiff.scope
      },
      availability,
      comparison: document2.comparison,
      diagnostics,
      projectionStamp,
      cacheKey
    };
  }
  __name(decodeRevisionInterdiff, "decodeRevisionInterdiff");
  function buildChangePageUrl(lens, query = {}) {
    const limit = query.limit ?? CHANGE_PAGE_LIMIT;
    if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
      throw new Error("Change page limit must be an integer from 1 through 100");
    }
    const params = new URLSearchParams({ limit: String(limit) });
    if (query.after !== void 0) {
      if (!query.after || new TextEncoder().encode(query.after).length > 4096) {
        throw new Error(
          "Change page continuation must be a non-empty opaque token"
        );
      }
      params.set("after", query.after);
    }
    if (query.q !== void 0) {
      const normalized = trimUnicodeWhitespace(query.q).toLowerCase();
      if (!normalized || new TextEncoder().encode(normalized).length > 256) {
        throw new Error(
          "Change page query must be non-empty and at most 256 bytes"
        );
      }
      params.set("q", normalized);
    }
    appendEnum(params, "topology", query.topology, TOPOLOGY_VALUES);
    appendEnum(params, "lifecycle", query.lifecycle, LIFECYCLE_VALUES);
    appendEnum(params, "attention", query.attention, ATTENTION_VALUES);
    appendEnum(params, "availability", query.availability, AVAILABILITY_VALUES);
    if (query.order !== void 0 && query.order !== "change_id_asc") {
      throw new Error("Change page order must be change_id_asc");
    }
    params.set("order", "change_id_asc");
    return `/api/v2/${lens}?${params}`;
  }
  __name(buildChangePageUrl, "buildChangePageUrl");
  function decodeReaderProfile(value) {
    const profile = object(value, "Inspector reader profile");
    const availability = profile.availability;
    const authorityCursor = profile.authorityCursor;
    const documents = profile.documents;
    const minimumReaderProfile = profile.minimumReaderProfile;
    const commitGraphStamp = profile.commitGraphStamp;
    if (profile.schema !== "pointbreak.inspect-reader-profile" || profile.version !== 1 || !isAvailability(availability) || !isRecord(authorityCursor) || !isDocumentMap(documents) || !sameDocumentMap(documents, CHANGE_READER_DOCUMENTS)) {
      throw new Error("incompatible Inspector reader profile");
    }
    if (availability === "ready" && (minimumReaderProfile !== CHANGE_READER_PROFILE || typeof commitGraphStamp !== "string" || commitGraphStamp.length === 0)) {
      throw new Error(
        "ready Inspector reader profile is missing capability or commit graph stamp"
      );
    }
    return {
      schema: "pointbreak.inspect-reader-profile",
      version: 1,
      availability,
      minimumReaderProfile: typeof minimumReaderProfile === "string" ? minimumReaderProfile : void 0,
      authorityCursor,
      commitGraphStamp: typeof commitGraphStamp === "string" ? commitGraphStamp : void 0,
      documents
    };
  }
  __name(decodeReaderProfile, "decodeReaderProfile");
  function decodeChangePage(value, expected) {
    const page = object(value, `${expected.lens} Change page`);
    const expectedSchema = expected.lens === "changes" ? "pointbreak.inspect-changes-page" : "pointbreak.inspect-attention";
    const expectedVersion = expected.lens === "changes" ? 1 : 2;
    const stamp = page.projectionStamp;
    const changes = page.changes;
    const diagnostics = page.diagnostics;
    const presentations = page.presentations;
    if (page.schema !== expectedSchema || page.version !== expectedVersion || !nonEmptyString(stamp) || !Array.isArray(changes) || expected.bounded && changes.length > 100 || !changes.every((change) => isChangeSummary(change, stamp)) || !isStrictlyAscending(changes.map((change) => change.changeId)) || new Set(changes.map((change) => change.changeId)).size !== changes.length || diagnostics !== void 0 && !isStringArray(diagnostics) || presentations !== void 0 && !isPresentations(presentations, changes)) {
      throw new Error(`invalid ${expected.lens} Change page DTO`);
    }
    const next = page.next;
    if (next !== void 0 && next !== null && !nonEmptyString(next)) {
      throw new Error("invalid Change page next continuation");
    }
    if (expected.bounded && next === void 0)
      throw new Error("bounded Change page is missing next continuation");
    const common = {
      changes,
      diagnostics,
      presentations,
      projectionStamp: stamp,
      next: next ?? null
    };
    return expected.lens === "changes" ? {
      schema: "pointbreak.inspect-changes-page",
      version: 1,
      ...common
    } : {
      schema: "pointbreak.inspect-attention",
      version: 2,
      ...common
    };
  }
  __name(decodeChangePage, "decodeChangePage");
  function requireCoherentGeneration(changes, attention) {
    if (changes.projectionStamp !== attention.projectionStamp) {
      throw new Error("Change documents do not form one coherent generation");
    }
  }
  __name(requireCoherentGeneration, "requireCoherentGeneration");
  function sameProfileGeneration(initial, postflight) {
    return initial.availability === postflight.availability && initial.minimumReaderProfile === postflight.minimumReaderProfile && initial.commitGraphStamp === postflight.commitGraphStamp && sameDocumentMap(initial.documents, postflight.documents) && canonicalJson(initial.authorityCursor) === canonicalJson(postflight.authorityCursor);
  }
  __name(sameProfileGeneration, "sameProfileGeneration");
  function trimUnicodeWhitespace(value) {
    return value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
  }
  __name(trimUnicodeWhitespace, "trimUnicodeWhitespace");
  function appendEnum(params, name, value, values) {
    if (value === void 0) return;
    if (!values.has(value)) throw new Error(`invalid Change page ${name}`);
    params.set(name, value);
  }
  __name(appendEnum, "appendEnum");
  function isAvailability(value) {
    return value === "migration_required" || value === "migration_in_progress" || value === "ready";
  }
  __name(isAvailability, "isAvailability");
  function isChangeSummary(value, stamp) {
    if (!isRecord(value)) return false;
    return nonEmptyString(value.changeId) && (value.declarationState === "authoritative" || value.declarationState === "incomplete" || value.declarationState === "conflicted") && isStringArray(value.titleAssertions) && typeof value.memberCount === "number" && Number.isSafeInteger(value.memberCount) && value.memberCount >= 0 && isOneOf(value.topology, TOPOLOGY_VALUES) && isOneOf(value.lifecycle, LIFECYCLE_VALUES) && isOneOf(value.attentionSummary, ATTENTION_VALUES) && isOneOf(value.availabilitySummary, AVAILABILITY_VALUES) && value.projectionStamp === stamp && Array.isArray(value.currentRevisionRefs) && value.currentRevisionRefs.every(isRevisionRef) && uniqueRevisionKeys(value.currentRevisionRefs).size === value.currentRevisionRefs.length && (value.diagnostics === void 0 || isStringArray(value.diagnostics));
  }
  __name(isChangeSummary, "isChangeSummary");
  function isClaimSupport(value) {
    return isRecord(value) && nonEmptyString(value.eventId) && nonEmptyString(value.actorId) && optionalString(value.trackId);
  }
  __name(isClaimSupport, "isClaimSupport");
  function isChangeMemberRevisions(value) {
    return Array.isArray(value) && value.every(
      (member) => isRecord(member) && isRevisionRef(member.revision) && isStringArray(member.supportingClaimIds)
    );
  }
  __name(isChangeMemberRevisions, "isChangeMemberRevisions");
  function isUnavailableChangeMemberRevisions(value) {
    return Array.isArray(value) && value.every(
      (member) => isRecord(member) && nonEmptyString(member.revisionId) && (member.reason === "invalid_revision_id" || member.reason === "invalid_object_artifact_content_hash") && isStringArray(member.supportingClaimIds)
    );
  }
  __name(isUnavailableChangeMemberRevisions, "isUnavailableChangeMemberRevisions");
  function isMembershipClaims(value, changeId) {
    return Array.isArray(value) && value.every(
      (claim) => isRecord(claim) && nonEmptyString(claim.claimId) && claim.changeId === changeId && nonEmptyString(claim.revisionId) && Array.isArray(claim.supports) && claim.supports.every(isClaimSupport) && Array.isArray(claim.withdrawals) && claim.withdrawals.every(isClaimSupport) && typeof claim.active === "boolean" && isStringArray(claim.diagnostics)
    );
  }
  __name(isMembershipClaims, "isMembershipClaims");
  function isClaimWithdrawals(value) {
    return Array.isArray(value) && value.every(
      (withdrawal) => isRecord(withdrawal) && nonEmptyString(withdrawal.claimId) && Array.isArray(withdrawal.supports) && withdrawal.supports.every(isClaimSupport) && isStringArray(withdrawal.diagnostics)
    );
  }
  __name(isClaimWithdrawals, "isClaimWithdrawals");
  function isChangeLinks(value) {
    return Array.isArray(value) && value.every(
      (link) => isRecord(link) && nonEmptyString(link.leftChangeId) && nonEmptyString(link.rightChangeId) && nonEmptyString(link.relation)
    );
  }
  __name(isChangeLinks, "isChangeLinks");
  function isEffectiveSupersedes(value) {
    return Array.isArray(value) && value.every(
      (edge) => Array.isArray(edge) && edge.length === 2 && isRevisionRef(edge[0]) && isRevisionRef(edge[1])
    );
  }
  __name(isEffectiveSupersedes, "isEffectiveSupersedes");
  function isRevisionQualifications(value, currentRevisionRefs) {
    if (!Array.isArray(value)) return false;
    const qualifications = [];
    for (const candidate of value) {
      if (!isRecord(candidate) || !isRevisionRef(candidate.revision)) {
        return false;
      }
      const revision = candidate.revision;
      if (typeof candidate.qualified !== "boolean" || !currentRevisionRefs.some((current) => sameRevision(current, revision)))
        return false;
      qualifications.push({
        revision,
        qualified: candidate.qualified
      });
    }
    return sameRevisionSet(
      qualifications.map((qualification) => qualification.revision),
      currentRevisionRefs
    );
  }
  __name(isRevisionQualifications, "isRevisionQualifications");
  function sameRevisionSet(left, right) {
    const leftKeys = uniqueRevisionKeys(left);
    const rightKeys = uniqueRevisionKeys(right);
    return leftKeys.size === left.length && rightKeys.size === right.length && leftKeys.size === rightKeys.size && [...leftKeys].every((key) => rightKeys.has(key));
  }
  __name(sameRevisionSet, "sameRevisionSet");
  function isPresentations(value, changes) {
    if (!isRecord(value)) return false;
    const summaries = new Map(
      changes.map((change) => [change.changeId, change])
    );
    if (Object.keys(value).length !== summaries.size) return false;
    return Object.entries(value).every(([changeId, presentation]) => {
      const change = summaries.get(changeId);
      if (change === void 0 || !isRecord(presentation) || !Array.isArray(presentation.currentRevisions) || !presentation.currentRevisions.every(isPresentationRevision)) {
        return false;
      }
      const expected = uniqueRevisionKeys(change.currentRevisionRefs);
      const actual = uniqueRevisionKeys(
        presentation.currentRevisions.map((candidate) => candidate.revision)
      );
      return expected.size === change.currentRevisionRefs.length && actual.size === presentation.currentRevisions.length && expected.size === actual.size && [...expected].every((key) => actual.has(key));
    });
  }
  __name(isPresentations, "isPresentations");
  function isPresentationRevision(value) {
    return isRecord(value) && isRevisionRef(value.revision) && (value.summarySource === "revision_proposal_summary" && nonEmptyString(value.revisionProposalSummary) || value.summarySource === "absent" && value.revisionProposalSummary === void 0);
  }
  __name(isPresentationRevision, "isPresentationRevision");
  function isRevisionRef(value) {
    return isRecord(value) && nonEmptyString(value.revisionId) && nonEmptyString(value.objectArtifactContentHash);
  }
  __name(isRevisionRef, "isRevisionRef");
  function uniqueRevisionKeys(revisions) {
    return new Set(
      revisions.map(
        (revision) => `${revision.revisionId}\0${revision.objectArtifactContentHash}`
      )
    );
  }
  __name(uniqueRevisionKeys, "uniqueRevisionKeys");
  function isRelationClaim(value, changeId) {
    return isRecord(value) && nonEmptyString(value.claimId) && value.changeId === changeId && typeof value.active === "boolean" && isRevisionRef(value.successor) && isRevisionRef(value.predecessor) && Array.isArray(value.supports) && value.supports.every(isClaimSupport) && Array.isArray(value.withdrawals) && value.withdrawals.every(isClaimSupport) && isStringArray(value.diagnostics);
  }
  __name(isRelationClaim, "isRelationClaim");
  function isFactPresentation(value) {
    return isRecord(value) && nonEmptyString(value.factId) && nonEmptyString(value.family) && isRevisionRef(value.originRevision) && (value.target === void 0 || isFactTarget(value.target)) && (value.contextChangeId === void 0 || nonEmptyString(value.contextChangeId)) && (value.presentedInRevision === void 0 || isRevisionRef(value.presentedInRevision)) && (value.portRelation === void 0 || value.portRelation === "context_only" || value.portRelation === "reanchored_as" || value.portRelation === "carried_open_as" || value.portRelation === "resolved_by") && nonEmptyString(value.actorId) && (value.trackId === void 0 || nonEmptyString(value.trackId)) && isOneOf(value.revisionCurrency, REVISION_CURRENCY_VALUES) && isOneOf(value.familyState, FACT_FAMILY_STATE_VALUES) && isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES);
  }
  __name(isFactPresentation, "isFactPresentation");
  function isFactTarget(value) {
    if (!isRecord(value) || !nonEmptyString(value.revisionId)) return false;
    if (value.kind === "revision") return true;
    if (value.kind === "file") return nonEmptyString(value.filePath);
    if (value.kind === "range") {
      return nonEmptyString(value.filePath) && (value.side === "old" || value.side === "new") && Number.isSafeInteger(value.startLine) && value.startLine > 0 && Number.isSafeInteger(value.endLine) && value.endLine >= value.startLine;
    }
    if (value.kind === "observation") return nonEmptyString(value.observationId);
    if (value.kind === "input_request")
      return nonEmptyString(value.inputRequestId);
    if (value.kind === "assessment") return nonEmptyString(value.assessmentId);
    return value.kind === "event" && nonEmptyString(value.eventId);
  }
  __name(isFactTarget, "isFactTarget");
  function uniqueFactPresentationIds(facts) {
    return new Set(facts.map((fact) => fact.factId)).size === facts.length;
  }
  __name(uniqueFactPresentationIds, "uniqueFactPresentationIds");
  function isResourceProjection(value) {
    return isRecord(value) && typeof value.includeBody === "boolean" && (value.trackId === void 0 || nonEmptyString(value.trackId));
  }
  __name(isResourceProjection, "isResourceProjection");
  function isCapturedReviewSnapshot(value, expectedContentHash, expectedObjectId) {
    if (!isRecord(value) || value.schema !== "pointbreak.review-snapshot" || value.version !== 1 || value.contentHash !== expectedContentHash || !isRecord(value.snapshot)) {
      return false;
    }
    return nonEmptyString(value.snapshot.review_id) && value.snapshot.object_id === expectedObjectId && Array.isArray(value.snapshot.files);
  }
  __name(isCapturedReviewSnapshot, "isCapturedReviewSnapshot");
  function isFactContentPresentations(value) {
    return isRecord(value) && Object.values(value).every(
      (presentation) => isRecord(presentation) && (presentation.contentType === "text/plain" || presentation.contentType === "text/markdown") && (presentation.bodyContentState === "present" || presentation.bodyContentState === "suppressed_present" || presentation.bodyContentState === "physically_removed") && isFactContent(presentation.content)
    );
  }
  __name(isFactContentPresentations, "isFactContentPresentations");
  function sameFactIds(facts, content) {
    const expected = new Set(facts.map((fact) => fact.factId));
    const actual = Object.keys(content);
    return expected.size === facts.length && expected.size === actual.length && actual.every((factId) => expected.has(factId));
  }
  __name(sameFactIds, "sameFactIds");
  function isRevisionResource(value) {
    try {
      decodeRevisionResource(value);
      return true;
    } catch {
      return false;
    }
  }
  __name(isRevisionResource, "isRevisionResource");
  function isFactPortPresentations(value, changeId, facts, selectedRevision) {
    if (!Array.isArray(value) || !value.every(isFactPortPresentation))
      return false;
    if (new Set(value.map((port) => port.portId)).size !== value.length)
      return false;
    return value.every(
      (port) => (port.contextChangeId === void 0 || port.contextChangeId === changeId) && port.sourceEventIds.length > 0 && new Set(port.sourceEventIds).size === port.sourceEventIds.length && port.trackId !== void 0 && (port.applicability !== "applicable" || applicableFactPortHasExactEndpoints(port, facts, selectedRevision))
    );
  }
  __name(isFactPortPresentations, "isFactPortPresentations");
  function factRefId(fact) {
    return fact.kind === "observation" ? fact.observationId ?? "" : fact.inputRequestId ?? "";
  }
  __name(factRefId, "factRefId");
  function applicableFactPortHasExactEndpoints(port, facts, selectedRevision) {
    if (!sameRevision(port.targetRevision, selectedRevision)) return false;
    const matchingOrigin = facts.filter(
      (fact) => fact.factId === factRefId(port.originFact) && fact.family === port.originFact.kind && sameRevision(fact.originRevision, port.originRevision) && fact.presentedInRevision !== void 0 && sameRevision(fact.presentedInRevision, selectedRevision)
    );
    if (matchingOrigin.length !== 1) return false;
    const targetFact = port.targetFact;
    if (targetFact === void 0) return true;
    return facts.filter(
      (fact) => fact.factId === factRefId(targetFact) && fact.family === targetFact.kind && sameRevision(fact.originRevision, selectedRevision)
    ).length === 1;
  }
  __name(applicableFactPortHasExactEndpoints, "applicableFactPortHasExactEndpoints");
  function isFactPortPresentation(value) {
    return isRecord(value) && nonEmptyString(value.portId) && isRevisionRef(value.originRevision) && isFactRef(value.originFact) && isRevisionRef(value.targetRevision) && (value.relation === "context_only" || value.relation === "reanchored_as" || value.relation === "carried_open_as" || value.relation === "resolved_by") && (value.targetFact === void 0 || isFactRef(value.targetFact)) && optionalString(value.rationaleContentHash) && optionalString(value.contextChangeId) && nonEmptyString(value.actorId) && nonEmptyString(value.trackId) && isStringArray(value.sourceEventIds) && (value.applicability === "applicable" || value.applicability === "conflicted" || value.applicability === "unavailable") && isStringArray(value.diagnostics);
  }
  __name(isFactPortPresentation, "isFactPortPresentation");
  function isFactRef(value) {
    if (!isRecord(value)) return false;
    if (value.kind === "observation") {
      return nonEmptyString(value.observationId) && value.inputRequestId === void 0;
    }
    if (value.kind === "input_request") {
      return nonEmptyString(value.inputRequestId) && value.observationId === void 0;
    }
    return false;
  }
  __name(isFactRef, "isFactRef");
  function isFactContent(value) {
    if (!isRecord(value)) return false;
    switch (value.kind) {
      case "observation":
        return nonEmptyString(value.title) && optionalString(value.body);
      case "input_request":
        return nonEmptyString(value.title) && optionalString(value.body) && nonEmptyString(value.status) && (value.responses === void 0 || Array.isArray(value.responses) && value.responses.every(isFactResponse));
      case "assessment":
        return nonEmptyString(value.assessment) && optionalString(value.summary);
      case "validation":
        return nonEmptyString(value.checkName) && optionalString(value.command) && nonEmptyString(value.status) && optionalString(value.summary);
      default:
        return false;
    }
  }
  __name(isFactContent, "isFactContent");
  function isFactResponse(value) {
    return isRecord(value) && nonEmptyString(value.responseId) && nonEmptyString(value.outcome) && optionalString(value.reason) && (value.contentType === "text/plain" || value.contentType === "text/markdown") && (value.bodyContentState === "present" || value.bodyContentState === "suppressed_present" || value.bodyContentState === "physically_removed") && isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES);
  }
  __name(isFactResponse, "isFactResponse");
  function isAssociation(value) {
    return isRecord(value) && value.schema === "pointbreak.review-association-comparison" && value.version === 1 && isOneOf(value.state, ASSOCIATION_STATE_VALUES) && isOneOf(value.proofAvailability, ASSOCIATION_PROOF_VALUES) && isRecord(value.comparison) && isRevisionRef(value.comparison.revision) && nonEmptyString(value.comparison.associationId) && nonEmptyString(value.comparison.commitOid) && nonEmptyString(value.comparison.comparisonBase) && nonEmptyString(value.comparison.viewKind) && optionalString(value.comparison.proofRef) && isStringArray(value.diagnostics) && nonEmptyString(value.cacheKey);
  }
  __name(isAssociation, "isAssociation");
  function sameRevision(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameRevision, "sameRevision");
  function isStrictlyAscending(values) {
    return values.every((value, index) => {
      const previous = values[index - 1];
      return index === 0 || previous !== void 0 && previous < value;
    });
  }
  __name(isStrictlyAscending, "isStrictlyAscending");
  function isOneOf(value, values) {
    return typeof value === "string" && values.has(value);
  }
  __name(isOneOf, "isOneOf");
  function isDocumentMap(value) {
    return isRecord(value) && Object.values(value).every((version) => Number.isInteger(version));
  }
  __name(isDocumentMap, "isDocumentMap");
  function sameDocumentMap(left, right) {
    const leftEntries = Object.entries(left).sort(
      ([a], [b]) => a.localeCompare(b)
    );
    const rightEntries = Object.entries(right).sort(
      ([a], [b]) => a.localeCompare(b)
    );
    return leftEntries.length === rightEntries.length && leftEntries.every(
      ([schema, version], index) => schema === rightEntries[index]?.[0] && version === rightEntries[index]?.[1]
    );
  }
  __name(sameDocumentMap, "sameDocumentMap");
  function canonicalJson(value) {
    if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
    if (isRecord(value)) {
      return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
    }
    return JSON.stringify(value);
  }
  __name(canonicalJson, "canonicalJson");
  function object(value, name) {
    if (!isRecord(value)) throw new Error(`invalid ${name} DTO`);
    return value;
  }
  __name(object, "object");
  function isRecord(value) {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }
  __name(isRecord, "isRecord");
  function nonEmptyString(value) {
    return typeof value === "string" && value.length > 0;
  }
  __name(nonEmptyString, "nonEmptyString");
  function optionalString(value) {
    return value === void 0 || typeof value === "string";
  }
  __name(optionalString, "optionalString");
  function isStringArray(value) {
    return Array.isArray(value) && value.every((item) => typeof item === "string");
  }
  __name(isStringArray, "isStringArray");

  // src/change-inspector-reading.ts
  function sameExactRevision(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameExactRevision, "sameExactRevision");
  function encoded(value) {
    return encodeURIComponent(value);
  }
  __name(encoded, "encoded");
  function revisionPath(changeId, revision) {
    return `/api/v2/changes/${encoded(changeId)}/revisions/${encoded(revision.revisionId)}?artifactHash=${encoded(revision.objectArtifactContentHash)}`;
  }
  __name(revisionPath, "revisionPath");
  function resourcePath(changeId, revision) {
    return `/api/v2/changes/${encoded(changeId)}/revisions/${encoded(revision.revisionId)}/resource?artifactHash=${encoded(revision.objectArtifactContentHash)}`;
  }
  __name(resourcePath, "resourcePath");
  function assertStamp(stamp, expected, surface) {
    if (stamp !== expected) {
      throw new Error(
        `${surface} projection stamp does not match the staged Change generation`
      );
    }
  }
  __name(assertStamp, "assertStamp");
  function assertRevisionDetail(document2, route, stamp) {
    if (document2.changeId !== route.changeId) {
      throw new Error(
        "contextual Revision detail Change ID does not match its exact route"
      );
    }
    if (!sameExactRevision(document2.revision, route.revision)) {
      throw new Error(
        "contextual Revision detail does not match its exact route"
      );
    }
    if (!sameExactRevision(
      document2.exactRevisionDocument.resource.revision,
      route.revision
    )) {
      throw new Error(
        "embedded captured resource does not match its exact Revision route"
      );
    }
    if (document2.factPresentations.some(
      (fact) => fact.contextChangeId !== route.changeId || fact.presentedInRevision !== void 0 && !sameExactRevision(fact.presentedInRevision, route.revision)
    )) {
      throw new Error(
        "fact presentation does not match its Change and exact Revision context"
      );
    }
    if (document2.factPorts.some(
      (port) => !sameExactRevision(port.targetRevision, route.revision)
    )) {
      throw new Error("fact port does not target the selected exact Revision");
    }
    if (document2.associations.some(
      (association) => !sameExactRevision(association.comparison.revision, route.revision)
    )) {
      throw new Error(
        "association comparison does not target the selected exact Revision"
      );
    }
    if (document2.exactRevisionDocument.projectionStamp !== document2.projectionStamp) {
      throw new Error(
        "embedded captured resource is from another projection stamp"
      );
    }
    assertStamp(document2.projectionStamp, stamp, "contextual Revision detail");
  }
  __name(assertRevisionDetail, "assertRevisionDetail");
  async function loadChangeInspectorReading(route, expectedProjectionStamp) {
    if (route.kind === "change") {
      const document3 = decodeChangeDetail(
        await fetchChangeInspectorJSON(
          `/api/v2/changes/${encoded(route.changeId)}`
        )
      );
      if (document3.summary.changeId !== route.changeId) {
        throw new Error("Change detail does not match its route");
      }
      assertStamp(
        document3.projectionStamp,
        expectedProjectionStamp,
        "Change detail"
      );
      return { kind: "change", document: document3 };
    }
    if (route.kind === "revision" || route.kind === "association") {
      const document3 = decodeChangeRevisionDetail(
        await fetchChangeInspectorJSON(
          revisionPath(route.changeId, route.revision)
        )
      );
      assertRevisionDetail(document3, route, expectedProjectionStamp);
      return { kind: route.kind, document: document3 };
    }
    if (route.kind === "resource") {
      const document3 = decodeRevisionResource(
        await fetchChangeInspectorJSON(
          resourcePath(route.changeId, route.revision)
        )
      );
      if (!sameExactRevision(document3.resource.revision, route.revision)) {
        throw new Error(
          "captured resource does not match its exact Revision route"
        );
      }
      assertStamp(
        document3.projectionStamp,
        expectedProjectionStamp,
        "captured resource"
      );
      return { kind: "resource", document: document3 };
    }
    const params = new URLSearchParams({
      fromArtifactHash: route.from.objectArtifactContentHash,
      toArtifactHash: route.to.objectArtifactContentHash
    });
    const document2 = decodeRevisionInterdiff(
      await fetchChangeInspectorJSON(
        `/api/v2/changes/${encoded(route.changeId)}/interdiff/${encoded(route.from.revisionId)}/${encoded(route.to.revisionId)}?${params}`
      )
    );
    if (!sameExactRevision(document2.interdiff.from, route.from) || !sameExactRevision(document2.interdiff.to, route.to)) {
      throw new Error(
        "ordered Revision interdiff does not match its exact route"
      );
    }
    assertStamp(
      document2.projectionStamp,
      expectedProjectionStamp,
      "Revision interdiff"
    );
    return { kind: "interdiff", document: document2 };
  }
  __name(loadChangeInspectorReading, "loadChangeInspectorReading");

  // src/change-inspector-cards.ts
  function words(value) {
    return value.replaceAll("_", " ");
  }
  __name(words, "words");
  function shortExact(revision) {
    const revisionId = revision.revisionId.length > 24 ? `${revision.revisionId.slice(0, 24)}…` : revision.revisionId;
    const artifact = revision.objectArtifactContentHash.length > 18 ? `${revision.objectArtifactContentHash.slice(0, 18)}…` : revision.objectArtifactContentHash;
    return `${revisionId} · ${artifact}`;
  }
  __name(shortExact, "shortExact");
  function exactRevisionAccessibleIdentity(revision) {
    return `exact Revision ${revision.revisionId}; artifact ${revision.objectArtifactContentHash}`;
  }
  __name(exactRevisionAccessibleIdentity, "exactRevisionAccessibleIdentity");
  function changeCardPresentation(summary, presentation) {
    const byExactIdentity = new Map(
      (presentation?.currentRevisions ?? []).map((entry) => [
        `${entry.revision.revisionId}\0${entry.revision.objectArtifactContentHash}`,
        entry
      ])
    );
    const peers = summary.currentRevisionRefs.map((revision) => {
      const entry = byExactIdentity.get(
        `${revision.revisionId}\0${revision.objectArtifactContentHash}`
      );
      const summaryLabel = entry?.summarySource === "revision_proposal_summary" ? entry.revisionProposalSummary : void 0;
      return {
        revision,
        label: summaryLabel ? `Current Revision — ${summaryLabel}` : `Current Revision — ${shortExact(revision)}`,
        accessibleName: summaryLabel ? `Current Revision — ${summaryLabel}; ${exactRevisionAccessibleIdentity(revision)}` : `Current Revision — ${exactRevisionAccessibleIdentity(revision)}`,
        copyText: `${revision.revisionId} ${revision.objectArtifactContentHash}`
      };
    });
    const currentRevisionName = peers.length === 0 ? "Current Revision unavailable" : peers.length === 1 ? peers[0].accessibleName : `Current Revisions — ${peers.map(
      (peer) => peer.accessibleName.replace(/^Current Revision — /, "")
    ).join("; ")}`;
    return {
      changeId: summary.changeId,
      accessibleName: `${currentRevisionName}; Change ${summary.changeId}`,
      badges: [
        summary.topology,
        summary.lifecycle,
        summary.attentionSummary,
        summary.availabilitySummary
      ].map(words),
      peers
    };
  }
  __name(changeCardPresentation, "changeCardPresentation");

  // src/classNames.ts
  var CLASS = {
    // App chrome, master-detail panes, lens containers, and shared chips.
    units: "units",
    timeline: "timeline",
    empty: "empty",
    badge: "badge",
    tierMedium: "tier-medium",
    body: "body",
    title: "title",
    time: "time",
    eventDate: "event-date",
    rail: "rail",
    meta: "meta",
    type: "type",
    typeCount: "type-count",
    code: "code",
    dot: "dot",
    kv: "kv",
    ghost: "ghost",
    actions: "actions",
    timelineShell: "timeline-shell",
    timelineNewPill: "timeline-new-pill",
    // (The app-shell store-identity chip + detail popover is static markup in
    // index.html — `store-identity*` classes live there and in app.css, not here —
    // and its rows are `renderIdentity`-filled <dt>/<dd> styled via element selectors.
    // Issue #391.)
    // Fact cards (observation / input-request / assessment / validation / note).
    annoGroup: "anno-group",
    annoHead: "anno-head",
    annoLoc: "anno-loc",
    annoSummary: "anno-summary",
    annoTime: "anno-time",
    annoTitle: "anno-title",
    annoTrack: "anno-track",
    actorAttribution: "actor-attribution",
    factBodyRemoved: "fact-body-removed",
    factRel: "fact-rel",
    factResponse: "fact-response",
    factResponses: "fact-responses",
    factStaleContext: "fact-stale-context",
    factStatus: "fact-status",
    outcome: "outcome",
    advisoryNote: "advisory-note",
    validationNote: "validation-note",
    validationContinuity: "validation-continuity",
    validationContinuityNeutral: "validation-continuity-neutral",
    validationContinuityOutstanding: "validation-continuity-outstanding",
    readback: "readback",
    readbackRow: "readback-row",
    readerScopeNote: "reader-scope-note",
    rawEvent: "raw-event",
    rawEventActions: "raw-event-actions",
    // The current-assessment verdict block.
    verdictStatus: "verdict-status",
    verdictSummary: "verdict-summary",
    verdictValue: "verdict-value",
    // The advisory endorsement readback.
    endorseAttrs: "endorse-attrs",
    endorseLabel: "endorse-label",
    endorseList: "endorse-list",
    endorseWho: "endorse-who",
    endorsements: "endorsements",
    endorsementsLabel: "endorsements-label",
    // The revision-overview summary line.
    overviewAssessment: "overview-assessment",
    overviewCue: "overview-cue",
    overviewHistoryCue: "overview-history-cue",
    overviewCues: "overview-cues",
    overviewLabel: "overview-label",
    overviewLatest: "overview-latest",
    overviewMain: "overview-main",
    overviewMuted: "overview-muted",
    revisionDiagnostic: "revision-diagnostic",
    overviewStat: "overview-stat",
    overviewStats: "overview-stats",
    overviewSummary: "overview-summary",
    // The annotated snapshot diff: files, rows, and the navigator.
    dfileBody: "dfile-body",
    dfileHead: "dfile-head",
    dfileNotes: "dfile-notes",
    dfileSummary: "dfile-summary",
    dhunk: "dhunk",
    diffBtn: "diff-btn",
    diffAnchorReason: "diff-anchor-reason",
    diffDecisionContext: "diff-decision-context",
    diffDecisionContextNav: "diff-decision-context-nav",
    diffFactVicinity: "diff-fact-vicinity",
    diffFileNotice: "diff-file-notice",
    diffNavFact: "diff-nav-fact",
    diffNavFile: "diff-nav-file",
    diffNavFiles: "diff-nav-files",
    diffNavReason: "diff-nav-reason",
    diffNavSummary: "diff-nav-summary",
    diffUnanchored: "diff-unanchored",
    diffUnanchoredFacts: "diff-unanchored-facts",
    dpath: "dpath",
    drow: "drow",
    drowMeta: "drow-meta",
    dtext: "dtext",
    emph: "emph",
    ln: "ln",
    sign: "sign",
    // Revision list, supersession badges, and the laid-out DAG.
    unitCard: "unit-card",
    unitPage: "unit-page",
    unitPageTitle: "unit-page-title",
    supersessionBadges: "supersession-badges",
    competing: "competing",
    revisionSupersession: "revision-supersession",
    revisionHeads: "revision-heads",
    revisionSelf: "revision-self",
    dagEdge: "dag-edge",
    dagArrowHead: "dag-arrow-head",
    dagArrowHeadTraced: "dag-arrow-head-traced",
    revisionDag: "revision-dag",
    factDag: "fact-dag",
    head: "head",
    stale: "stale",
    superseded: "superseded",
    supersedes: "supersedes",
    upEmpty: "up-empty",
    upIdentity: "up-identity",
    upStat: "up-stat",
    upStats: "up-stats",
    // The applied-filter chip row (the toolbar's pure view of filterText).
    filterChips: "filter-chips",
    filterChipRemove: "filter-chip-remove",
    // The type facet section (the Timeline-only ?type= page-set control): static
    // container/list classes in index.html; rows are emitted via typeFacetRowClass.
    typeFacet: "type-facet",
    typeFacetMenu: "type-facet-menu",
    // The search-bar suggestion popover: static list container in index.html;
    // the rows are emitted via suggestionClass below.
    filterSuggestions: "filter-suggestions",
    suggestion: "suggestion",
    suggestionActive: "suggestion-active",
    // The command palette.
    cmdEmpty: "cmd-empty",
    cmdGroup: "cmd-group",
    cmdHint: "cmd-hint",
    cmdLabel: "cmd-label",
    // The attention lens: tiered cards over the outstanding review state.
    attentionCard: "attention-card",
    attentionTier: "attention-tier",
    attentionEmpty: "attention-empty",
    attentionOrderLabel: "attention-order-label",
    attentionKind: "attention-kind",
    attentionMeta: "attention-meta",
    attentionFreshness: "attention-freshness",
    attentionFocus: "attention-focus",
    attentionDelta: "attention-delta",
    // The attention tab's judgment-queue count badge (absent when both tiers are
    // empty) and the muted advisory count beside the needs-input number.
    attentionBadge: "attention-badge",
    attentionBadgeSecondary: "attention-badge-secondary",
    // The detail page's per-revision outstanding set (the scoped attention read);
    // absent when nothing is outstanding on the shown revision.
    outstandingSet: "outstanding-set",
    // Copyable CLI command handoffs (workflow-handoff.ts): the block, its label,
    // the command code, the visible placeholder marker, the clipboard-only copy
    // control, and the detail page's stage-template section host.
    workflowHandoff: "workflow-handoff",
    workflowHandoffLabel: "workflow-handoff-label",
    workflowCommand: "workflow-command",
    workflowPlaceholder: "workflow-placeholder",
    workflowCopy: "workflow-copy",
    workflowHandoffs: "workflow-handoffs"
  };
  var ANNO_KINDS = [
    "observation",
    "assessment",
    "input-request",
    "validation"
  ];
  var DIFF_ROW_KINDS = ["added", "removed", "context"];
  var TOKEN_KINDS = [
    "keyword",
    "string",
    "comment",
    "number",
    "type",
    "function",
    "constant",
    "operator",
    "punctuation",
    "variable"
  ];
  var DIFF_FILE_STATUSES = [
    "added",
    "deleted",
    "modified",
    "renamed",
    "copied"
  ];
  var VERIFY_STATUSES = [
    "valid",
    "invalid",
    "unsigned",
    "untrusted_key"
  ];
  var ENDORSE_CLASSES = [
    "endorsement-trusted",
    "ambiguous_endorser",
    "unknown_endorser"
  ];
  var VERDICT_ASSESSMENTS = [
    "accepted",
    "accepted_with_follow_up",
    "ambiguous",
    "needs_changes",
    "needs_clarification",
    "unassessed"
  ];
  var FACT_STATUSES = [
    "accepted",
    "accepted_with_follow_up",
    "ambiguous",
    "current",
    "errored",
    "failed",
    "needs_changes",
    "needs_clarification",
    "open",
    "passed",
    "replaced",
    "resolved",
    "responded",
    "skipped",
    "stale",
    "superseded",
    "unassessed"
  ];
  var REF_ID_PREFIXES = [
    "input-request-response",
    "input-request",
    "obs",
    "assess",
    "rev",
    "evt",
    "validation",
    "obj",
    "engagement",
    "checkpoint",
    "task-attempt",
    "assoc-commit",
    "assoc-ref",
    "withdraw-commit",
    "withdraw-ref"
  ];
  var REF_KINDS = [
    ...REF_ID_PREFIXES,
    "hash",
    "commit",
    "track",
    "actor"
  ];
  var annoContainerClass = /* @__PURE__ */ __name((kind) => `anno anno-${kind}`, "annoContainerClass");
  var annoKindClass = /* @__PURE__ */ __name((kind) => `anno-kind anno-kind-${kind}`, "annoKindClass");
  var drowClass = /* @__PURE__ */ __name((kind, noted) => `drow drow-${kind}${noted ? " drow-noted" : ""}`, "drowClass");
  var tokClass = /* @__PURE__ */ __name((kind) => `tok tok-${kind}`, "tokClass");
  var diffStatusClass = /* @__PURE__ */ __name((status) => `dstatus s-${status}`, "diffStatusClass");
  var verifyClass = /* @__PURE__ */ __name((status) => `verify verify-${status}`, "verifyClass");
  var endorseClass = /* @__PURE__ */ __name((cls) => `endorse endorse-${cls}`, "endorseClass");
  var verdictClass = /* @__PURE__ */ __name((assessment) => `verdict verdict-${assessment}`, "verdictClass");
  var factStatusClass = /* @__PURE__ */ __name((status) => `fact-status ${status}`, "factStatusClass");
  var refClass = /* @__PURE__ */ __name((kind) => `ref ref-${kind}`, "refClass");
  var dfileClass = /* @__PURE__ */ __name((lowSignal) => `dfile${lowSignal ? " dfile-lowsignal" : ""}`, "dfileClass");
  var dagNodeClass = /* @__PURE__ */ __name((o) => `dag-node${o.isHead ? " head" : ""}${o.isSuperseded ? " superseded" : ""}`, "dagNodeClass");
  var bodyClass = /* @__PURE__ */ __name((base, markdown) => `${base}${markdown ? " markdown-body" : ""}`, "bodyClass");
  var cmdItemClass = /* @__PURE__ */ __name((active2) => `cmd-item${active2 ? " active" : ""}`, "cmdItemClass");
  var filterChipClass = /* @__PURE__ */ __name((negated) => `filter-chip${negated ? " filter-chip-negated" : ""}`, "filterChipClass");
  var typeFacetRowClass = /* @__PURE__ */ __name((enabled) => `type-facet-row${enabled ? "" : " type-facet-row-off"}`, "typeFacetRowClass");
  var suggestionClass = /* @__PURE__ */ __name((active2) => `suggestion${active2 ? " suggestion-active" : ""}`, "suggestionClass");
  var tokensOf = /* @__PURE__ */ __name((classStrings) => classStrings.flatMap((s) => s.split(" ")), "tokensOf");
  var ALL_EMITTABLE_CLASSES = [
    ...new Set(
      tokensOf([
        ...Object.values(CLASS),
        ...ANNO_KINDS.map((k) => annoContainerClass(k)),
        ...ANNO_KINDS.map((k) => annoKindClass(k)),
        ...DIFF_ROW_KINDS.map((k) => drowClass(k, true)),
        ...TOKEN_KINDS.map((k) => tokClass(k)),
        ...DIFF_FILE_STATUSES.map((s) => diffStatusClass(s)),
        ...VERIFY_STATUSES.map((s) => verifyClass(s)),
        ...ENDORSE_CLASSES.map((c) => endorseClass(c)),
        ...VERDICT_ASSESSMENTS.map((a) => verdictClass(a)),
        ...FACT_STATUSES.map((s) => factStatusClass(s)),
        ...REF_KINDS.map((k) => refClass(k)),
        dfileClass(true),
        filterChipClass(true),
        typeFacetRowClass(true),
        typeFacetRowClass(false),
        suggestionClass(true),
        dagNodeClass({ isHead: true, isSuperseded: true }),
        bodyClass("anno-body", true),
        bodyClass("verdict-summary", true),
        cmdItemClass(true)
      ])
    )
  ];

  // src/escape.ts
  var ENTITIES = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;"
  };
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (char) => ENTITIES[char]);
  }
  __name(escapeHtml, "escapeHtml");

  // src/format.ts
  var RFC3339_UTC = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?Z$/;
  function parseRfc3339UtcMillis(value) {
    const match = value.match(RFC3339_UTC);
    if (!match) return null;
    const [
      ,
      yearText,
      monthText,
      dayText,
      hourText,
      minuteText,
      secondText,
      fraction
    ] = match;
    const year = Number(yearText);
    const month = Number(monthText);
    const day = Number(dayText);
    const hour = Number(hourText);
    const minute = Number(minuteText);
    const second = Number(secondText);
    const leapYear = year % 4 === 0 && year % 100 !== 0 || year % 400 === 0;
    const daysInMonth = [
      31,
      leapYear ? 29 : 28,
      31,
      30,
      31,
      30,
      31,
      31,
      30,
      31,
      30,
      31
    ];
    if (month < 1 || month > 12 || day < 1 || day > daysInMonth[month - 1] || hour > 23 || minute > 59 || second > 60) {
      return null;
    }
    const millis = Number((fraction ?? "").padEnd(3, "0").slice(0, 3));
    const date = /* @__PURE__ */ new Date(0);
    date.setUTCFullYear(year, month - 1, day);
    date.setUTCHours(hour, minute, Math.min(second, 59), millis);
    return date.getTime() + (second === 60 ? 1e3 : 0);
  }
  __name(parseRfc3339UtcMillis, "parseRfc3339UtcMillis");
  function parseMs(occurredAt) {
    if (typeof occurredAt !== "string") return null;
    if (occurredAt.startsWith("unix-ms:")) {
      const unixMillis = occurredAt.match(/^unix-ms:([+-]?\d+)$/);
      return unixMillis ? Number(unixMillis[1]) : null;
    }
    if (/^\d{4}-\d{2}-\d{2}T/.test(occurredAt))
      return parseRfc3339UtcMillis(occurredAt);
    const match = occurredAt.match(/(\d+)\s*$/);
    return match ? Number(match[1]) : null;
  }
  __name(parseMs, "parseMs");
  function fmtDateTime(occurredAt) {
    const ms = parseMs(occurredAt);
    if (ms == null) return occurredAt || "";
    return new Date(ms).toLocaleString([], { hour12: false });
  }
  __name(fmtDateTime, "fmtDateTime");

  // src/refs.ts
  function shortRef(id) {
    const value = String(id);
    let match = value.match(
      /^([a-z][a-z-]*):(?:git:|worktree:)?sha256:([0-9a-f]{6,})$/i
    );
    if (match) return `${match[1]}:${match[2].slice(0, 8)}`;
    match = value.match(/^sha256:([0-9a-f]{8,})$/i);
    if (match) return `sha256:${match[1].slice(0, 8)}`;
    if (/^[0-9a-f]{40}$/i.test(value)) return value.slice(0, 10);
    return value;
  }
  __name(shortRef, "shortRef");
  var NON_CLICKABLE_KINDS = /* @__PURE__ */ new Set([
    "validation",
    "obj",
    "engagement",
    "checkpoint",
    "task-attempt",
    "assoc-commit",
    "assoc-ref",
    "withdraw-commit",
    "withdraw-ref"
  ]);
  function refInfo(token) {
    const match = token.match(
      /^([a-z][a-z-]*):(?:git:|worktree:)?sha256:[0-9a-f]+$/i
    );
    if (match) {
      const kind = match[1].toLowerCase();
      return { kind, clickable: !NON_CLICKABLE_KINDS.has(kind) };
    }
    if (/^sha256:[0-9a-f]+$/i.test(token))
      return { kind: "hash", clickable: false };
    if (/^[0-9a-f]{40}$/i.test(token))
      return { kind: "commit", clickable: false };
    if (/^(agent|human):[a-z0-9][a-z0-9_-]*$/i.test(token)) {
      return { kind: "track", clickable: true };
    }
    return null;
  }
  __name(refInfo, "refInfo");
  var REF_RE = new RegExp(
    `\\b(?:${REF_ID_PREFIXES.join("|")}):(?:git:|worktree:)?sha256:[0-9a-f]{6,}\\b|(?<!:)\\bsha256:[0-9a-f]{16,}\\b|\\b[0-9a-f]{40}\\b|\\b(?:agent|human):[a-z0-9][a-z0-9_-]*\\b`,
    "gi"
  );
  function linkifyEscaped(escaped, opts = {}) {
    const tabIndex = typeof opts === "object" ? opts.tabIndex ?? 0 : 0;
    return escaped.replace(REF_RE, (token) => {
      const info = refInfo(token);
      if (!info) return token;
      const display = escapeHtml(shortRef(token));
      if (!info.clickable) {
        return `<span class="${refClass(info.kind)}" title="${escapeHtml(token)}">${display}</span>`;
      }
      return `<span class="${refClass(info.kind)}" role="link" tabindex="${tabIndex}" data-ref-kind="${info.kind}" data-ref-id="${escapeHtml(token)}" title="${escapeHtml(token)}">${display}</span>`;
    });
  }
  __name(linkifyEscaped, "linkifyEscaped");
  function linkify(text, opts = {}) {
    return linkifyEscaped(escapeHtml(String(text ?? "")), opts);
  }
  __name(linkify, "linkify");
  function actorChip(actorId, opts = {}) {
    if (!actorId) return "";
    const tabIndex = typeof opts === "object" ? opts.tabIndex ?? 0 : opts;
    const display = escapeHtml(actorId);
    return `<span class="${refClass("actor")}" role="link" tabindex="${tabIndex}" data-ref-kind="actor" data-ref-id="${escapeHtml(actorId)}" title="filter to ${escapeHtml(actorId)}">${display}</span>`;
  }
  __name(actorChip, "actorChip");
  function isMarkdownContentType(contentType) {
    return contentType === "text/markdown";
  }
  __name(isMarkdownContentType, "isMarkdownContentType");
  function safeMarkdownHref(href) {
    const raw = String(href ?? "").trim();
    if (/^(https?:|mailto:)/i.test(raw) || raw.startsWith("#"))
      return escapeHtml(raw);
    return "";
  }
  __name(safeMarkdownHref, "safeMarkdownHref");

  // src/markdown.ts
  function renderBodyContent(text, contentType) {
    if (!text) return "";
    const cls = bodyClass("anno-body", isMarkdownContentType(contentType));
    return `<div class="${cls}">${renderContentHtml(text, contentType)}</div>`;
  }
  __name(renderBodyContent, "renderBodyContent");
  function renderContentHtml(text, contentType) {
    return isMarkdownContentType(contentType) ? renderMarkdown(text) : linkify(text);
  }
  __name(renderContentHtml, "renderContentHtml");
  function renderMarkdown(text) {
    const lines = String(text ?? "").replace(/\r\n?/g, "\n").split("\n");
    const out = [];
    let paragraph = [];
    let listKind = null;
    let listItems = [];
    const flushParagraph = /* @__PURE__ */ __name(() => {
      if (!paragraph.length) return;
      out.push(`<p>${renderMarkdownInline(paragraph.join(" "))}</p>`);
      paragraph = [];
    }, "flushParagraph");
    const flushList = /* @__PURE__ */ __name(() => {
      if (!listKind) return;
      out.push(
        `<${listKind}>${listItems.map((item) => `<li>${renderMarkdownInline(item)}</li>`).join("")}</${listKind}>`
      );
      listKind = null;
      listItems = [];
    }, "flushList");
    const flushBlocks = /* @__PURE__ */ __name(() => {
      flushParagraph();
      flushList();
    }, "flushBlocks");
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const fence = line.match(/^\s*```/);
      if (fence) {
        flushBlocks();
        const code = [];
        i++;
        while (i < lines.length && !/^\s*```/.test(lines[i])) {
          code.push(lines[i]);
          i++;
        }
        out.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
        continue;
      }
      if (!line.trim()) {
        flushBlocks();
        continue;
      }
      const heading = line.match(/^(#{1,6})\s+(.+)$/);
      if (heading) {
        flushBlocks();
        const level = heading[1].length;
        out.push(
          `<h${level}>${renderMarkdownInline(heading[2].trim())}</h${level}>`
        );
        continue;
      }
      const unordered = line.match(/^\s*[-*]\s+(.+)$/);
      if (unordered) {
        flushParagraph();
        if (listKind && listKind !== "ul") flushList();
        listKind = "ul";
        listItems.push(unordered[1]);
        continue;
      }
      const ordered = line.match(/^\s*\d+[.)]\s+(.+)$/);
      if (ordered) {
        flushParagraph();
        if (listKind && listKind !== "ol") flushList();
        listKind = "ol";
        listItems.push(ordered[1]);
        continue;
      }
      if (listKind) flushList();
      paragraph.push(line.trim());
    }
    flushBlocks();
    return out.join("");
  }
  __name(renderMarkdown, "renderMarkdown");
  function renderMarkdownInline(text) {
    const placeholders = [];
    const stash = /* @__PURE__ */ __name((html2) => {
      const token = `\0MD${placeholders.length}\0`;
      placeholders.push([token, html2]);
      return token;
    }, "stash");
    let html = escapeHtml(String(text ?? ""));
    html = protectBackslashEscapes(html, stash, (character) => character === "`");
    html = html.replace(
      /`([^`]+)`/g,
      (_, code) => stash(`<code>${code}</code>`)
    );
    html = protectBackslashEscapes(html, stash);
    html = html.replace(
      /\[([^\]]+)\]\(([^)\s]+)\)/g,
      (_, label, href) => {
        const safe = safeMarkdownHref(href);
        const labelHtml = renderMarkdownInline(label);
        return safe ? stash(
          `<a href="${safe}" target="_blank" rel="noreferrer">${labelHtml}</a>`
        ) : labelHtml;
      }
    );
    html = html.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>").replace(/\*([^*]+)\*/g, "<em>$1</em>");
    html = linkifyEscaped(html);
    for (const [token, replacement] of placeholders.reverse()) {
      html = html.split(token).join(replacement);
    }
    return html;
  }
  __name(renderMarkdownInline, "renderMarkdownInline");
  function protectBackslashEscapes(html, stash, shouldProtect = isAsciiPunctuation) {
    let protectedHtml = "";
    for (let index = 0; index < html.length; index++) {
      const character = html[index];
      const escaped = html[index + 1];
      if (character === "\\" && escaped && shouldProtect(escaped)) {
        protectedHtml += stash(escaped);
        index++;
      } else {
        protectedHtml += character;
      }
    }
    return protectedHtml;
  }
  __name(protectBackslashEscapes, "protectBackslashEscapes");
  function isAsciiPunctuation(character) {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint >= 33 && codePoint <= 47 || codePoint >= 58 && codePoint <= 64 || codePoint >= 91 && codePoint <= 96 || codePoint >= 123 && codePoint <= 126;
  }
  __name(isAsciiPunctuation, "isAsciiPunctuation");

  // src/types.ts
  var TYPES = [
    { id: "review_initialized", label: "init", color: "var(--evt-init)" },
    { id: "work_object_proposed", label: "capture", color: "var(--evt-capture)" },
    {
      id: "review_observation_recorded",
      label: "observation",
      color: "var(--evt-observation)"
    },
    {
      id: "review_assessment_recorded",
      label: "assessment",
      color: "var(--evt-assessment)"
    },
    { id: "input_request_opened", label: "request", color: "var(--evt-request)" },
    {
      id: "input_request_responded",
      label: "response",
      color: "var(--evt-response)"
    },
    { id: "review_note_imported", label: "note", color: "var(--evt-note)" },
    {
      id: "validation_check_recorded",
      label: "validation",
      color: "var(--evt-validation)"
    }
  ];
  var TYPE_MAP = Object.fromEntries(TYPES.map((type) => [type.id, type]));
  var VERIFICATION_LABELS = {
    valid: "signature valid",
    invalid: "signature invalid",
    untrusted_key: "untrusted key",
    unsigned: "unsigned"
  };
  var ENDORSEMENT_LABELS = {
    "endorsement-trusted": "trusted endorsement",
    unknown_endorser: "unknown endorser",
    ambiguous_endorser: "ambiguous endorser"
  };
  var ASSESSMENT_LABELS = {
    accepted: "accepted",
    accepted_with_follow_up: "accepted-with-follow-up",
    needs_changes: "needs-changes",
    needs_clarification: "needs-clarification"
  };
  var REVISION_ATTENTION_VALUES = [
    "open-request",
    "unassessed",
    "validation-context",
    "follow-up",
    "stale-fact"
  ];
  var DEFAULT_OPEN_FILES = 10;
  var LARGE_FILE_ROWS = 500;

  // src/projection.ts
  function verificationChip(status) {
    if (!status) return "";
    const label = VERIFICATION_LABELS[status] || status;
    return `<span class="${verifyClass(escapeHtml(status))}" title="advisory signature readback — reader-relative, never gates a write">${escapeHtml(label)}</span>`;
  }
  __name(verificationChip, "verificationChip");
  function endorserDisplay(actorId) {
    return actorId.replace(/^actor:git-(email|name):/, "");
  }
  __name(endorserDisplay, "endorserDisplay");
  function endorsementRow(en) {
    const cls = en.classification || "";
    const label = ENDORSEMENT_LABELS[cls] || cls;
    const parts = [
      `<span class="${CLASS.endorseLabel}">${escapeHtml(label)}</span>`
    ];
    if (en.endorser) {
      parts.push(
        `<span class="${CLASS.endorseWho}">${escapeHtml(endorserDisplay(en.endorser))}</span>`
      );
    }
    const attrs = en.endorserAttributes || {};
    const attrBits = [];
    if (attrs.kind) attrBits.push(attrs.kind);
    const roles = attrs.roles || [];
    if (roles.length) attrBits.push(roles.join(", "));
    if (attrBits.length) {
      parts.push(
        `<span class="${CLASS.endorseAttrs}">${escapeHtml(attrBits.join(" · "))}</span>`
      );
    }
    return `<li class="${endorseClass(escapeHtml(cls))}">${parts.join(" ")}</li>`;
  }
  __name(endorsementRow, "endorsementRow");
  function endorsementsBlock(endorsements) {
    const list = endorsements || [];
    if (!list.length) return "";
    const rows = list.map(endorsementRow).join("");
    return `<div class="${CLASS.endorsements}" title="advisory endorsement readback — reader-relative, never gates a write">
    <span class="${CLASS.endorsementsLabel}">endorsements</span>
    <ul class="${CLASS.endorseList}">${rows}</ul>
  </div>`;
  }
  __name(endorsementsBlock, "endorsementsBlock");
  function assessmentDisplayLabel(value) {
    return ASSESSMENT_LABELS[value] || value || "";
  }
  __name(assessmentDisplayLabel, "assessmentDisplayLabel");
  var [
    ATTENTION_OPEN_REQUEST,
    ATTENTION_UNASSESSED,
    ATTENTION_VALIDATION_CONTEXT,
    ATTENTION_FOLLOW_UP,
    ATTENTION_STALE_FACT
  ] = REVISION_ATTENTION_VALUES;

  // src/cards.ts
  var VALIDATION_DISPOSITION_LABELS = {
    outstanding: "outstanding",
    current: "current result",
    resolved_by_later_pass: "resolved by strictly later pass",
    historical: "historical",
    skipped: "skipped"
  };
  function renderActorAttribution(label, writer) {
    const actorId = writer?.actorId ?? "";
    if (!actorId) return "";
    return `<span class="${CLASS.actorAttribution}">${label} ${actorChip(actorId)}</span>`;
  }
  __name(renderActorAttribution, "renderActorAttribution");
  function renderRecordedTime(createdAt) {
    if (!createdAt) return "";
    return `<span class="${CLASS.annoTime}" title="${escapeHtml(createdAt)}">${escapeHtml(fmtDateTime(createdAt))}</span>`;
  }
  __name(renderRecordedTime, "renderRecordedTime");
  function targetLabel(t) {
    const tt = t ?? {};
    switch (tt.kind) {
      case "range":
        return `${escapeHtml(tt.filePath)}:${tt.startLine}-${tt.endLine ?? tt.startLine} (${escapeHtml(tt.side || "new")})`;
      case "file":
        return escapeHtml(tt.filePath || "");
      case "revision":
        return "whole revision";
      case "observation":
        return `→ ${linkify(tt.observationId)}`;
      case "input_request":
        return `→ ${linkify(tt.inputRequestId)}`;
      case "assessment":
        return `→ ${linkify(tt.assessmentId)}`;
      case "event":
        return `→ ${linkify(tt.eventId)}`;
      default:
        return escapeHtml(tt.kind || "");
    }
  }
  __name(targetLabel, "targetLabel");
  function removedBodyCue(state) {
    if (state !== "suppressed_present" && state !== "physically_removed") {
      return null;
    }
    const title = state === "suppressed_present" ? "removal recorded; bytes still stored until compact" : "removed; bytes swept from the store";
    return `<div class="${CLASS.factBodyRemoved}" title="${title}">content removed</div>`;
  }
  __name(removedBodyCue, "removedBodyCue");
  function factCard(kind, opts) {
    const tags = (opts.tags || []).filter(Boolean).map((t) => `<span class="${CLASS.badge}">${escapeHtml(t)}</span>`).join(" ");
    const body = removedBodyCue(opts.bodyContentState) ?? renderBodyContent(opts.body, opts.bodyContentType);
    const annotationId = opts.annotationId ? ` data-anno="${escapeHtml(opts.annotationId)}"` : "";
    return `<div class="${annoContainerClass(kind)}"${annotationId}>
    <div class="${CLASS.annoHead}">
      <span class="${annoKindClass(kind)}">${kind}</span>
      <span class="${CLASS.annoTrack}">${escapeHtml(opts.track || "")}</span>
      ${renderActorAttribution("writer", opts.writer)}
      <span class="${CLASS.annoTitle}">${linkify(opts.title || "")}</span>
      ${opts.status ? `<span class="${factStatusClass(escapeHtml(opts.status))}">${escapeHtml(opts.status)}</span>` : ""}
      ${opts.target ? `<span class="${CLASS.annoLoc}">${opts.target}</span>` : ""}
      ${tags}
      ${opts.verify || ""}
      ${renderRecordedTime(opts.createdAt)}
    </div>
    ${body}
    ${opts.endorsements || ""}
    ${opts.extra || ""}</div>`;
  }
  __name(factCard, "factCard");
  function renderObservationCard(o) {
    const supersedes = o.supersedes ?? [];
    const extra = supersedes.length ? `<div class="${CLASS.factRel}">supersedes ${supersedes.map(linkify).join(", ")}</div>` : "";
    return factCard("observation", {
      annotationId: o.id,
      track: o.trackId,
      title: o.title,
      status: o.status,
      target: targetLabel(o.target),
      tags: o.tags,
      body: o.body,
      bodyContentType: o.bodyContentType,
      bodyContentState: o.bodyContentState,
      createdAt: o.createdAt,
      verify: verificationChip(o.verificationStatus ?? ""),
      endorsements: endorsementsBlock(o.endorsements),
      writer: o.writer,
      extra
    });
  }
  __name(renderObservationCard, "renderObservationCard");
  function renderInputRequestResponse(r) {
    const reason = removedBodyCue(r.reasonContentState) ?? (r.reason ? renderBodyContent(r.reason, r.reasonContentType) : "");
    return `<div class="${CLASS.factResponse}">
    <div class="${CLASS.annoHead}">
      <span class="${CLASS.outcome}">${escapeHtml(r.outcome)}</span>
      ${r.id ? `<span class="${CLASS.annoLoc}">${linkify(r.id)}</span>` : ""}
      ${renderActorAttribution("answered by", r.writer)}
      ${verificationChip(r.verificationStatus ?? "")}
      ${renderRecordedTime(r.createdAt)}
    </div>
    ${reason}
    ${endorsementsBlock(r.endorsements)}
  </div>`;
  }
  __name(renderInputRequestResponse, "renderInputRequestResponse");
  function renderInputRequestCard(ir) {
    const responses = (ir.responses ?? []).map(renderInputRequestResponse).join("");
    return factCard("input-request", {
      annotationId: ir.id,
      track: ir.trackId,
      title: ir.title,
      status: ir.status,
      target: targetLabel(ir.target),
      tags: [ir.mode, ir.reasonCode],
      body: ir.body,
      bodyContentType: ir.bodyContentType,
      bodyContentState: ir.bodyContentState,
      createdAt: ir.createdAt,
      verify: verificationChip(ir.verificationStatus ?? ""),
      endorsements: endorsementsBlock(ir.endorsements),
      writer: ir.writer,
      extra: responses ? `<div class="${CLASS.factResponses}">${responses}</div>` : ""
    });
  }
  __name(renderInputRequestCard, "renderInputRequestCard");
  function renderAssessmentCard(a) {
    const rel = [];
    const replaces = a.replaces ?? [];
    const relatedObservations = a.relatedObservations ?? [];
    const relatedInputRequests = a.relatedInputRequests ?? [];
    if (replaces.length) rel.push(`replaces ${replaces.map(linkify).join(", ")}`);
    if (relatedObservations.length) {
      rel.push(`re ${relatedObservations.map(linkify).join(", ")}`);
    }
    if (relatedInputRequests.length) {
      rel.push(`re ${relatedInputRequests.map(linkify).join(", ")}`);
    }
    return factCard("assessment", {
      annotationId: a.id,
      track: a.trackId,
      title: assessmentDisplayLabel(a.assessment ?? ""),
      status: a.status,
      target: targetLabel(a.target),
      body: a.summary,
      bodyContentType: a.summaryContentType,
      bodyContentState: a.summaryContentState,
      createdAt: a.createdAt,
      verify: verificationChip(a.verificationStatus ?? ""),
      endorsements: endorsementsBlock(a.endorsements),
      writer: a.writer,
      extra: rel.length ? `<div class="${CLASS.factRel}">${rel.join(" · ")}</div>` : ""
    });
  }
  __name(renderAssessmentCard, "renderAssessmentCard");
  function renderValidationCheckCard(v, disposition) {
    const rel = [];
    const logs = v.logArtifactContentHashes ?? [];
    if (v.command) rel.push(escapeHtml(v.command));
    if (logs.length) rel.push(`logs ${logs.map(linkify).join(", ")}`);
    const continuity = disposition ? `<div class="${CLASS.validationContinuity} ${disposition === "outstanding" ? CLASS.validationContinuityOutstanding : CLASS.validationContinuityNeutral}" title="server-projected validation continuity">${escapeHtml(VALIDATION_DISPOSITION_LABELS[disposition])}</div>` : "";
    const related = rel.length ? `<div class="${CLASS.factRel}">${rel.join(" · ")}</div>` : "";
    return factCard("validation", {
      annotationId: v.id,
      track: v.trackId,
      title: v.checkName,
      status: v.status,
      // passed | failed | errored | skipped → .fact-status.<status>
      target: targetLabel(v.target),
      tags: [v.trigger, v.exitCode != null ? `exit ${v.exitCode}` : null],
      body: v.summary || "",
      bodyContentType: v.summaryContentType,
      bodyContentState: v.summaryContentState,
      createdAt: v.completedAt || v.createdAt,
      verify: verificationChip(v.verificationStatus ?? ""),
      endorsements: endorsementsBlock(v.endorsements),
      writer: v.writer,
      extra: continuity + related
    });
  }
  __name(renderValidationCheckCard, "renderValidationCheckCard");

  // src/diff/highlight.ts
  function validChannel(spans, len) {
    let cursor = 0;
    for (const span of spans) {
      if (!Number.isInteger(span.start) || !Number.isInteger(span.end) || span.start < cursor || span.end < span.start || span.end > len) {
        return false;
      }
      cursor = span.end;
    }
    return true;
  }
  __name(validChannel, "validChannel");
  function segClass(kind, isEmph) {
    const parts = [
      kind ? tokClass(kind) : null,
      isEmph ? CLASS.emph : null
    ].filter(Boolean);
    return parts.length > 0 ? parts.join(" ") : null;
  }
  __name(segClass, "segClass");
  function highlightRowText(text, tokens, emphasis) {
    const toks = tokens && validChannel(tokens, text.length) ? tokens : [];
    const emph = emphasis && validChannel(emphasis, text.length) ? emphasis : [];
    if (toks.length === 0 && emph.length === 0) return escapeHtml(text);
    const points = [
      .../* @__PURE__ */ new Set([
        0,
        text.length,
        ...toks.flatMap((t) => [t.start, t.end]),
        ...emph.flatMap((e) => [e.start, e.end])
      ])
    ].sort((a, b) => a - b);
    let out = "";
    for (let i = 0; i + 1 < points.length; i++) {
      const a = points[i];
      const b = points[i + 1];
      if (a >= b) continue;
      const seg = escapeHtml(text.slice(a, b));
      const kind = toks.find((t) => t.start <= a && a < t.end)?.kind;
      const isEmph = emph.some((e) => e.start <= a && a < e.end);
      const cls = segClass(kind, isEmph);
      out += cls ? `<span class="${cls}">${seg}</span>` : seg;
    }
    return out;
  }
  __name(highlightRowText, "highlightRowText");

  // src/diff/render.ts
  function filePathLabel(f) {
    const oldp = f.old_path;
    const newp = f.new_path;
    return oldp && newp && oldp !== newp ? `${oldp} → ${newp}` : newp || oldp || "(unknown path)";
  }
  __name(filePathLabel, "filePathLabel");
  function fileRowCount(f) {
    return (f.hunks ?? []).reduce((n, h) => n + (h.rows ? h.rows.length : 0), 0);
  }
  __name(fileRowCount, "fileRowCount");
  function classifyLowSignal(f) {
    if (f.is_binary) return "binary";
    if (f.is_mode_only) return "mode change only";
    const hunks = f.hunks ?? [];
    const renamed = f.status === "renamed" || !!f.old_path && !!f.new_path && f.old_path !== f.new_path;
    if (renamed && !hunks.length) {
      return f.similarity != null ? `rename ${f.similarity}%` : "rename";
    }
    if (fileRowCount(f) > LARGE_FILE_ROWS) return "large file";
    return null;
  }
  __name(classifyLowSignal, "classifyLowSignal");
  function fileFactCount(f, anchored) {
    const oldp = f.old_path;
    const newp = f.new_path;
    let n = 0;
    for (const a of anchored) {
      const p = a.target?.filePath;
      if (p === newp || p === oldp) n += 1;
    }
    return n;
  }
  __name(fileFactCount, "fileFactCount");
  function fileForFact(files, filePath) {
    return files.find((f) => f.new_path === filePath || f.old_path === filePath) ?? null;
  }
  __name(fileForFact, "fileForFact");
  function rangeTouchesCapturedRows(a, file) {
    if (!file) return false;
    const t = a.target ?? {};
    if (t.kind !== "range" || t.startLine == null) return true;
    const start = t.startLine;
    const side = t.side === "old" ? "old" : "new";
    const end = t.endLine ?? start;
    for (const h of file.hunks ?? []) {
      for (const r of h.rows ?? []) {
        const line = side === "old" ? r.old_line : r.new_line;
        if (line != null && line >= start && line <= end) return true;
      }
    }
    return false;
  }
  __name(rangeTouchesCapturedRows, "rangeTouchesCapturedRows");
  function renderAnnotation(a, showLocation) {
    const target = showLocation && a.target?.filePath ? a.target : void 0;
    switch (a.kind) {
      case "observation":
        return renderObservationCard({
          id: a.id,
          trackId: a.track,
          title: a.title,
          status: a.status,
          target,
          tags: a.tags,
          body: a.body,
          bodyContentType: a.bodyContentType,
          bodyContentState: a.bodyContentState,
          createdAt: a.createdAt,
          verificationStatus: a.verificationStatus,
          endorsements: a.endorsements,
          supersedes: a.supersedes,
          writer: a.writer
        });
      case "input-request":
        return renderInputRequestCard({
          id: a.id,
          trackId: a.track,
          title: a.title,
          status: a.status,
          target,
          mode: a.mode,
          reasonCode: a.reasonCode,
          body: a.body,
          bodyContentType: a.bodyContentType,
          bodyContentState: a.bodyContentState,
          createdAt: a.createdAt,
          verificationStatus: a.verificationStatus,
          endorsements: a.endorsements,
          responses: a.responses,
          writer: a.writer
        });
      case "assessment":
        return renderAssessmentCard({
          id: a.id,
          trackId: a.track,
          assessment: a.assessment ?? a.title.replace(/^assessment:\s*/, "").trim(),
          status: a.status,
          target,
          summary: a.body,
          summaryContentType: a.bodyContentType,
          summaryContentState: a.bodyContentState,
          createdAt: a.createdAt,
          verificationStatus: a.verificationStatus,
          endorsements: a.endorsements,
          replaces: a.replaces,
          relatedObservations: a.relatedObservations,
          relatedInputRequests: a.relatedInputRequests,
          writer: a.writer
        });
      case "validation":
        return renderValidationCheckCard(
          {
            id: a.id,
            trackId: a.track,
            checkName: a.title,
            status: a.status,
            target,
            trigger: a.trigger,
            exitCode: a.exitCode,
            summary: a.body,
            summaryContentType: a.bodyContentType,
            summaryContentState: a.bodyContentState,
            completedAt: a.completedAt,
            createdAt: a.createdAt,
            verificationStatus: a.verificationStatus,
            endorsements: a.endorsements,
            command: a.command,
            logArtifactContentHashes: a.logArtifactContentHashes,
            writer: a.writer
          },
          a.continuity
        );
      default:
        return factCard(a.kind, {
          annotationId: a.id,
          track: a.track,
          title: a.title,
          status: a.status,
          body: a.body,
          bodyContentType: a.bodyContentType,
          bodyContentState: a.bodyContentState,
          createdAt: a.createdAt,
          writer: a.writer,
          tags: a.tags
        });
    }
  }
  __name(renderAnnotation, "renderAnnotation");
  function renderDiffFactVicinity(f, anchored) {
    const facts = anchored.filter((a) => {
      const p = a.target?.filePath;
      return p === f.new_path || p === f.old_path;
    });
    return `<div class="${CLASS.diffFactVicinity}" data-fact-vicinity="true">
    <p>Large annotated file: showing review facts first.</p>
    <button type="button" data-render-diff-file="true">Render all rows</button>
    ${facts.map((a) => renderAnnotation(a, true)).join("")}
  </div>`;
  }
  __name(renderDiffFactVicinity, "renderDiffFactVicinity");
  function renderDiffFileHeader(f, anchored, reason, open) {
    const n = fileFactCount(f, anchored);
    const summary = reason ? `<span class="${CLASS.dfileSummary}">${escapeHtml(reason)}</span>` : "";
    return `<header class="${CLASS.dfileHead}" role="button" tabindex="0" aria-expanded="${open}">
    <span class="${diffStatusClass(escapeHtml(f.status))}">${escapeHtml(f.status)}</span>
    <span class="${CLASS.dpath}">${escapeHtml(filePathLabel(f))}</span>${summary}
    ${n ? `<span class="${CLASS.dfileNotes}">${n} note${n === 1 ? "" : "s"}</span>` : ""}</header>`;
  }
  __name(renderDiffFileHeader, "renderDiffFileHeader");
  function renderDiffFileBody(f, anchored) {
    const oldp = f.old_path;
    const newp = f.new_path;
    const fileFacts = anchored.filter((a) => {
      const p = a.target?.filePath;
      return p === newp || p === oldp;
    });
    const rangeFacts = fileFacts.filter((a) => a.target?.kind === "range");
    const fileLevelFacts = fileFacts.filter((a) => a.target?.kind === "file");
    const emitted = /* @__PURE__ */ new Set();
    let html = "";
    for (const a of fileLevelFacts) {
      html += renderAnnotation(a, false);
      emitted.add(a.id);
    }
    for (const m of f.metadata_rows ?? []) {
      html += `<div class="${CLASS.drow} ${CLASS.drowMeta}"><span class="${CLASS.dtext}">${escapeHtml(m.text)}</span></div>`;
    }
    const factsByLine = /* @__PURE__ */ new Map();
    for (const a of rangeFacts) {
      const t = a.target ?? {};
      if (t.startLine == null) continue;
      const start = t.startLine;
      const side = t.side === "old" ? "old" : "new";
      const end = t.endLine ?? start;
      for (let line = start; line <= end; line++) {
        const key = `${side}:${line}`;
        const bucket = factsByLine.get(key);
        if (bucket) bucket.push(a);
        else factsByLine.set(key, [a]);
      }
    }
    const hunks = f.hunks ?? [];
    for (const h of hunks) {
      html += `<div class="${CLASS.dhunk}">${escapeHtml(h.header)}</div>`;
      for (const r of h.rows ?? []) {
        const matching = [];
        const seen = /* @__PURE__ */ new Set();
        const collect = /* @__PURE__ */ __name((key) => {
          const bucket = factsByLine.get(key);
          if (!bucket) return;
          for (const a of bucket) {
            if (!seen.has(a.id)) {
              seen.add(a.id);
              matching.push(a);
            }
          }
        }, "collect");
        if (r.old_line != null) collect(`old:${r.old_line}`);
        if (r.new_line != null) collect(`new:${r.new_line}`);
        const sign = r.kind === "added" ? "+" : r.kind === "removed" ? "-" : " ";
        const noted = matching.length > 0;
        const notedAttrs = noted ? ` data-anno="${escapeHtml(matching[0].id)}" tabindex="0" role="button"` : "";
        html += `<div class="${drowClass(escapeHtml(r.kind), noted)}"${notedAttrs}>
        <span class="${CLASS.ln}">${r.old_line ?? ""}</span>
        <span class="${CLASS.ln}">${r.new_line ?? ""}</span>
        <span class="${CLASS.sign}">${sign}</span>
        <span class="${CLASS.dtext}">${highlightRowText(r.text, r.tokens, r.emphasis)}</span></div>`;
        for (const a of matching) {
          if (!emitted.has(a.id)) {
            html += renderAnnotation(a, false);
            emitted.add(a.id);
          }
        }
      }
    }
    for (const a of rangeFacts) {
      if (!emitted.has(a.id)) {
        html += renderAnnotation(a, true);
        emitted.add(a.id);
      }
    }
    if (!hunks.length && !(f.metadata_rows ?? []).length) {
      if (!classifyLowSignal(f)) {
        html += `<div class="${CLASS.drow} ${CLASS.drowMeta}"><span class="${CLASS.dtext}">(no captured content)</span></div>`;
      }
    }
    return html;
  }
  __name(renderDiffFileBody, "renderDiffFileBody");
  function partitionAnnotations(files, annotations) {
    const anchored = [];
    const decisionContext = [];
    const unanchored = [];
    for (const annotation of annotations) {
      const target = annotation.target ?? {};
      if (annotation.kind === "validation") {
        decisionContext.push(annotation);
        continue;
      }
      if (target.kind !== "range" && target.kind !== "file") {
        decisionContext.push(annotation);
        continue;
      }
      if (!target.filePath) {
        unanchored.push(annotation);
        continue;
      }
      const file = fileForFact(files, target.filePath);
      if (!file || target.kind === "range" && !rangeTouchesCapturedRows(annotation, file)) {
        unanchored.push(annotation);
        continue;
      }
      anchored.push(annotation);
    }
    return { anchored, decisionContext, unanchored };
  }
  __name(partitionAnnotations, "partitionAnnotations");
  function renderDecisionContext(annotations) {
    if (!annotations.length) return "";
    return `<section class="${CLASS.diffDecisionContext}" aria-label="Decision context">
    <h2>Decision context (${annotations.length})</h2>
    <p>Review evidence and recorded assessments for this revision. Validation remains context only.</p>
    <div class="${CLASS.annoGroup}">${annotations.map((annotation) => renderAnnotation(annotation, true)).join("")}</div>
  </section>`;
  }
  __name(renderDecisionContext, "renderDecisionContext");
  function renderUnanchoredFacts(annotations, filePaths) {
    if (!annotations.length) return "";
    return `<section class="${CLASS.diffUnanchoredFacts}" aria-label="Unanchored facts">
    <h2>Unanchored facts (${annotations.length})</h2>
    <div class="${CLASS.annoGroup}">${annotations.map(
      (annotation) => `<div><p class="${CLASS.diffAnchorReason}">${escapeHtml(unanchoredReason(annotation, filePaths))}</p>${renderAnnotation(annotation, true)}</div>`
    ).join("")}</div>
  </section>`;
  }
  __name(renderUnanchoredFacts, "renderUnanchoredFacts");
  function renderDiff(snapshotId, artifact, annotations) {
    const annos = annotations ?? [];
    const files = artifact.snapshot?.files ?? [];
    const filePaths = /* @__PURE__ */ new Set();
    for (const f of files) {
      if (f.new_path) filePaths.add(f.new_path);
      if (f.old_path) filePaths.add(f.old_path);
    }
    const { anchored, decisionContext, unanchored } = partitionAnnotations(
      files,
      annos
    );
    const ctx = {
      snapshotId,
      files,
      anchored,
      decisionContext,
      unanchored,
      filePaths
    };
    const counts = {};
    for (const a of annos) {
      counts[a.kind] = (counts[a.kind] ?? 0) + 1;
    }
    const breakdown = Object.entries(counts).map(([k, n]) => `${n} ${k}${n === 1 ? "" : "s"}`).join(", ");
    let html = `<div class="${CLASS.annoSummary}">${annos.length} review fact${annos.length === 1 ? "" : "s"} on this revision${breakdown ? ` · ${breakdown}` : ""}${unanchored.length ? ` · ${unanchored.length} not anchored to a diff line` : ""}</div>`;
    html += renderDecisionContext(decisionContext);
    html += renderUnanchoredFacts(unanchored, filePaths);
    if (!files.length) {
      return {
        html: `${html}<p class="${CLASS.empty}">No files captured in this snapshot.</p>`,
        ctx
      };
    }
    let openBudget = DEFAULT_OPEN_FILES;
    html += files.map((f, i) => {
      const reason = classifyLowSignal(f);
      const annotated = fileFactCount(f, anchored) > 0;
      const annotatedLarge = annotated && fileRowCount(f) > LARGE_FILE_ROWS;
      const open = annotated && !annotatedLarge || (reason ? false : openBudget-- > 0);
      const expanded = annotatedLarge || open;
      const body = annotatedLarge ? renderDiffFactVicinity(f, anchored) : open ? renderDiffFileBody(f, anchored) : "";
      const lowAttr = reason ? ` data-lowsignal="${escapeHtml(reason)}"` : "";
      const bodyAttr = annotatedLarge ? ` data-fact-vicinity="true"` : open ? ` data-rendered="1"` : "";
      return `<section class="${dfileClass(!!reason)}" data-dfile="${i}" data-expanded="${expanded}"${lowAttr}>${renderDiffFileHeader(f, anchored, reason, expanded)}<div class="${CLASS.dfileBody}" data-dfile-body="${i}"${bodyAttr}>${body}</div></section>`;
    }).join("");
    return { html, ctx };
  }
  __name(renderDiff, "renderDiff");
  function unanchoredReason(a, filePaths) {
    const t = a.target ?? {};
    if (t.kind !== "range" && t.kind !== "file") {
      return "not a file or range target";
    }
    if (!t.filePath) return "target missing file path";
    if (t.kind === "range" && filePaths.has(t.filePath)) {
      return "line outside captured rows";
    }
    if (!filePaths.has(t.filePath)) return "file missing from snapshot";
    return "not anchored to a diff line";
  }
  __name(unanchoredReason, "unanchoredReason");

  // src/change-inspector-render.ts
  var FILTER_OPTIONS = [
    [
      "topology",
      [
        "initial",
        "replacement",
        "replacement_divergent",
        "consolidation",
        "parallel_current",
        "mixed",
        "incomplete",
        "cycle_conflicted"
      ]
    ],
    ["lifecycle", ["incomplete", "conflicted", "in_progress", "accepted"]],
    ["attention", ["clear", "in_progress", "incomplete", "conflicted"]],
    ["availability", ["available", "incomplete"]]
  ];
  function message(text) {
    const element = document.createElement("p");
    element.className = "empty";
    element.textContent = text;
    return element;
  }
  __name(message, "message");
  function setText(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.textContent = value;
  }
  __name(setText, "setText");
  function replaceMasterWith(...children) {
    const master = document.querySelector("#master");
    if (!master) return;
    delete master.dataset.changeListKey;
    master.replaceChildren(...children);
  }
  __name(replaceMasterWith, "replaceMasterWith");
  function replaceDetailWith(...children) {
    const detail = document.querySelector("#detail-body");
    if (!detail) return;
    delete detail.dataset.changeReadingKey;
    detail.replaceChildren(...children);
  }
  __name(replaceDetailWith, "replaceDetailWith");
  function prepareChangeInspectorShell(actions2) {
    document.querySelector("#view-controls")?.classList.remove("hidden");
    setText("#view-toggle", "View");
    document.querySelector("#view-order-section")?.classList.add("hidden");
    document.querySelector("#view-sort-section")?.classList.add("hidden");
    document.querySelector("#jump-latest")?.closest(".control-section")?.classList.add("hidden");
    document.querySelector("#derived-access-status")?.classList.add("hidden");
    document.querySelector("#follow-toggle")?.classList.add("hidden");
    const switcher = document.querySelector("#lens-switcher");
    if (switcher) {
      switcher.replaceChildren();
      for (const lens of ["changes", "attention"]) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "lens-tab";
        button.dataset.lens = lens;
        button.textContent = lens === "changes" ? "Changes" : "Attention";
        button.addEventListener("click", () => {
          const current = parseChangeInspectorRoute(location.hash || "#/changes");
          actions2.navigate({
            kind: "lens",
            lens,
            query: current.kind === "invalid" ? {} : { ...current.query, after: void 0 }
          });
        });
        switcher.append(button);
      }
    }
    const back = document.querySelector("#detail-back");
    if (back) {
      back.textContent = "‹ Changes";
    }
    const search = document.querySelector("#filter-text");
    if (search) search.placeholder = "Search Changes and current Revisions";
    const filterTypes = document.querySelector("#filter-types");
    if (filterTypes) {
      filterTypes.replaceChildren();
      const heading = document.createElement("h2");
      heading.id = "filter-types-label";
      heading.className = "control-heading";
      heading.textContent = "Change status";
      filterTypes.append(heading);
      for (const [name, values] of FILTER_OPTIONS) {
        const label = document.createElement("label");
        label.textContent = name.replaceAll("_", " ");
        const select = document.createElement("select");
        select.id = `change-filter-${name}`;
        for (const [labelText, value] of [
          ["Any", ""],
          ...values.map((value2) => [value2.replaceAll("_", " "), value2])
        ]) {
          const option = document.createElement("option");
          option.textContent = labelText;
          option.value = value;
          select.append(option);
        }
        select.addEventListener("change", () => {
          const current = parseChangeInspectorRoute(location.hash || "#/changes");
          const base = current.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : current;
          actions2.navigate({
            ...base,
            query: {
              ...base.query,
              after: void 0,
              [name]: select.value || void 0
            }
          });
        });
        label.append(select);
        filterTypes.append(label);
      }
    }
    const clear = document.querySelector("#filter-clear");
    if (clear) {
      clear.onclick = () => {
        const current = parseChangeInspectorRoute(location.hash || "#/changes");
        const base = current.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : current;
        actions2.navigate({
          ...base,
          // Clear only reader filters and the now-invalid continuation. Keep the
          // bounded page shape in the URL so reset does not silently change the
          // caller's explicit limit or stable ordering contract.
          query: {
            limit: base.query.limit,
            order: base.query.order
          }
        });
      };
    }
  }
  __name(prepareChangeInspectorShell, "prepareChangeInspectorShell");
  function copyExact(value) {
    if (navigator.clipboard) void navigator.clipboard.writeText(value);
  }
  __name(copyExact, "copyExact");
  function clearError() {
    const banner = document.querySelector("#error");
    if (!banner) return;
    banner.textContent = "";
    banner.classList.add("hidden");
  }
  __name(clearError, "clearError");
  function filterValues(query) {
    const values = [];
    if (query.q) values.push(["search", query.q]);
    for (const [name] of FILTER_OPTIONS) {
      const value = query[name];
      if (value) values.push([name, value]);
    }
    return values;
  }
  __name(filterValues, "filterValues");
  function syncFilterChrome(route) {
    if (route.kind === "invalid") return;
    const input = document.querySelector("#filter-text");
    if (input) input.value = route.query.q ?? "";
    for (const [name] of FILTER_OPTIONS) {
      const select = document.querySelector(
        `#change-filter-${name}`
      );
      if (select) select.value = route.query[name] ?? "";
    }
    const values = filterValues(route.query);
    const chips = document.querySelector("#filter-chips");
    if (chips) {
      chips.replaceChildren(
        ...values.map(([name, value]) => {
          const chip = document.createElement("span");
          chip.className = "badge";
          chip.textContent = `${name}: ${value.replaceAll("_", " ")}`;
          return chip;
        })
      );
    }
    document.querySelector("#filter-chips-empty")?.classList.toggle("hidden", values.length > 0);
    const toggle = document.querySelector("#filters-toggle");
    if (toggle)
      toggle.textContent = values.length ? `Filters · ${values.length}` : "Filters";
  }
  __name(syncFilterChrome, "syncFilterChrome");
  function detailHeading(text, level = 2) {
    const heading = document.createElement(`h${level}`);
    heading.textContent = text;
    return heading;
  }
  __name(detailHeading, "detailHeading");
  function detailLine(text, className) {
    const line = document.createElement("p");
    if (className) line.className = className;
    line.textContent = text;
    return line;
  }
  __name(detailLine, "detailLine");
  function shortExact2(revision) {
    return `${revision.revisionId} · ${revision.objectArtifactContentHash}`;
  }
  __name(shortExact2, "shortExact");
  function renderedFactBody(content, contentType) {
    const body = document.createElement("div");
    body.className = "anno-body";
    const text = content.kind === "observation" || content.kind === "input_request" ? content.body : content.kind === "assessment" || content.kind === "validation" ? content.summary : void 0;
    if (text) body.innerHTML = renderBodyContent(text, contentType);
    return body;
  }
  __name(renderedFactBody, "renderedFactBody");
  function renderFacts(reading, route, actions2) {
    const facts = document.createElement("section");
    facts.className = "detail-facts";
    facts.append(detailHeading("Facts", 3));
    const groups = /* @__PURE__ */ new Map();
    for (const fact of reading.document.factPresentations) {
      const family = groups.get(fact.family) ?? [];
      family.push(fact);
      groups.set(fact.family, family);
    }
    for (const [family, items] of groups) {
      const group = document.createElement("section");
      group.append(detailHeading(family.replaceAll("_", " "), 4));
      for (const fact of items) {
        const card = document.createElement("article");
        card.className = "unit-card";
        card.dataset.factId = fact.factId;
        card.tabIndex = -1;
        card.append(
          detailLine(fact.factId, "mono"),
          detailLine(
            `origin: ${shortExact2(fact.originRevision)} · context: ${fact.contextChangeId ?? "unavailable"} · currency: ${fact.revisionCurrency.replaceAll("_", " ")}`
          ),
          detailLine(
            `family: ${fact.familyState.replaceAll("_", " ")} · availability: ${fact.availability.replaceAll("_", " ")} · actor: ${fact.actorId}${fact.trackId ? ` · track: ${fact.trackId}` : ""}`
          )
        );
        const presentedInRevision = fact.presentedInRevision;
        if (presentedInRevision) {
          const applicablePort = reading.document.factPorts.find(
            (port) => port.applicability === "applicable" && port.originRevision.revisionId === fact.originRevision.revisionId && port.originRevision.objectArtifactContentHash === fact.originRevision.objectArtifactContentHash && port.targetRevision.revisionId === presentedInRevision.revisionId && port.targetRevision.objectArtifactContentHash === presentedInRevision.objectArtifactContentHash && factRefLabel(port.originFact) === factRefLabelFromFactId(fact)
          );
          card.append(
            detailLine(
              `presented in: ${shortExact2(presentedInRevision)} · port: ${fact.portRelation?.replaceAll("_", " ") ?? (applicablePort ? `${applicablePort.relation.replaceAll("_", " ")} (${applicablePort.portId})` : "see Fact ports")}`
            )
          );
        }
        const content = reading.document.factContentPresentations?.[fact.factId];
        if (content) {
          card.append(
            detailLine(
              `body: ${content.bodyContentState.replaceAll("_", " ")} · ${content.contentType}`
            ),
            renderedFactBody(content.content, content.contentType)
          );
        }
        const focus = document.createElement("button");
        focus.type = "button";
        focus.className = "ghost";
        focus.textContent = "Focus fact";
        focus.addEventListener(
          "click",
          () => actions2.navigate({
            kind: route.kind,
            changeId: route.changeId,
            revision: route.revision,
            query: queryForExactNavigation(route),
            focus: { factId: fact.factId }
          })
        );
        card.append(focus);
        group.append(card);
      }
      facts.append(group);
    }
    if (groups.size === 0) facts.append(message("No facts."));
    return facts;
  }
  __name(renderFacts, "renderFacts");
  function factRefLabel(fact) {
    return fact.kind === "observation" ? `observation: ${fact.observationId}` : `input request: ${fact.inputRequestId}`;
  }
  __name(factRefLabel, "factRefLabel");
  function factRefLabelFromFactId(fact) {
    return fact.family === "observation" ? `observation: ${fact.factId}` : fact.family === "input_request" ? `input request: ${fact.factId}` : "";
  }
  __name(factRefLabelFromFactId, "factRefLabelFromFactId");
  function renderFactPorts(reading) {
    const ports = document.createElement("section");
    ports.append(detailHeading("Fact ports", 3));
    for (const port of reading.document.factPorts) {
      const item = document.createElement("article");
      item.className = "unit-card";
      item.append(
        detailLine(port.portId, "mono"),
        detailLine(
          `origin: ${factRefLabel(port.originFact)} · ${shortExact2(port.originRevision)}`
        ),
        detailLine(
          `target: ${shortExact2(port.targetRevision)} · ${port.relation.replaceAll("_", " ")}`
        ),
        detailLine(
          `target fact: ${port.targetFact ? factRefLabel(port.targetFact) : "none"} · applicability: ${port.applicability.replaceAll("_", " ")}`
        ),
        detailLine(
          `actor: ${port.actorId}${port.trackId ? ` · track: ${port.trackId}` : ""}${port.contextChangeId ? ` · context: ${port.contextChangeId}` : ""}`
        )
      );
      if (port.rationaleContentHash)
        item.append(
          detailLine(`rationale: ${port.rationaleContentHash}`, "mono")
        );
      for (const diagnostic of port.diagnostics)
        item.append(detailLine(diagnostic));
      ports.append(item);
    }
    if (reading.document.factPorts.length === 0)
      ports.append(message("No fact ports."));
    return ports;
  }
  __name(renderFactPorts, "renderFactPorts");
  function openCapturedResource(route, actions2) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost";
    button.textContent = "Open authoritative captured diff";
    button.addEventListener(
      "click",
      () => actions2.navigate({
        kind: "resource",
        changeId: route.changeId,
        revision: route.revision,
        query: route.query,
        ...route.focus ? { focus: route.focus } : {}
      })
    );
    return button;
  }
  __name(openCapturedResource, "openCapturedResource");
  function renderAssociations(reading, route, actions2) {
    const section = document.createElement("section");
    section.append(detailHeading("Association comparisons", 3));
    for (const association of reading.document.associations) {
      const item = document.createElement("article");
      item.className = "unit-card";
      item.append(
        detailLine(association.comparison.associationId, "mono"),
        detailLine(
          `commit: ${association.comparison.commitOid} · base: ${association.comparison.comparisonBase}`
        ),
        detailLine(
          `view: ${association.comparison.viewKind} · state: ${association.state} · proof: ${association.proofAvailability}`
        )
      );
      if (association.comparison.proofRef)
        item.append(
          detailLine(`proof: ${association.comparison.proofRef}`, "mono")
        );
      for (const diagnostic of association.diagnostics)
        item.append(detailLine(diagnostic));
      item.append(openCapturedResource(route, actions2));
      section.append(item);
    }
    if (reading.document.associations.length === 0)
      section.append(message("No association comparisons."));
    return section;
  }
  __name(renderAssociations, "renderAssociations");
  function capturedDiffArtifact(documentValue) {
    if (typeof documentValue !== "object" || documentValue === null) return null;
    const documentRecord = documentValue;
    const snapshot2 = documentRecord.snapshot;
    if (typeof snapshot2 !== "object" || snapshot2 === null) return null;
    const files = snapshot2.files;
    return Array.isArray(files) ? documentValue : null;
  }
  __name(capturedDiffArtifact, "capturedDiffArtifact");
  function annotationTarget(target) {
    return {
      kind: target.kind,
      filePath: target.filePath,
      startLine: target.startLine,
      endLine: target.endLine,
      side: target.side,
      observationId: target.observationId,
      inputRequestId: target.inputRequestId,
      assessmentId: target.assessmentId,
      eventId: target.eventId
    };
  }
  __name(annotationTarget, "annotationTarget");
  function annotationBody(content) {
    return content.kind === "observation" || content.kind === "input_request" ? content.body : content.kind === "assessment" || content.kind === "validation" ? content.summary : void 0;
  }
  __name(annotationBody, "annotationBody");
  function annotationForFact(fact, presentation) {
    const content = presentation.content;
    const base = {
      id: fact.factId,
      kind: content.kind === "input_request" ? "input-request" : content.kind,
      title: content.kind === "assessment" ? `assessment: ${content.assessment}` : content.kind === "validation" ? content.checkName : content.title,
      track: fact.trackId ?? "untracked",
      body: annotationBody(content),
      bodyContentType: presentation.contentType,
      bodyContentState: presentation.bodyContentState,
      ...fact.target ? { target: annotationTarget(fact.target) } : {}
    };
    if (content.kind === "input_request") {
      base.status = content.status;
      base.responses = content.responses?.map((response) => ({
        id: response.responseId,
        outcome: response.outcome,
        reason: response.reason,
        reasonContentType: response.contentType,
        reasonContentState: response.bodyContentState,
        verificationStatus: response.availability
      }));
    } else if (content.kind === "assessment") {
      base.assessment = content.assessment;
      base.status = fact.familyState;
    } else if (content.kind === "validation") {
      base.status = content.status;
      base.command = content.command;
    }
    return base;
  }
  __name(annotationForFact, "annotationForFact");
  function annotationsForExactRevision(detail) {
    const annotations = [];
    for (const fact of detail.factPresentations) {
      const content = detail.factContentPresentations?.[fact.factId];
      if (fact.family !== content?.content.kind || fact.originRevision.revisionId !== detail.revision.revisionId || fact.originRevision.objectArtifactContentHash !== detail.revision.objectArtifactContentHash || !content) {
        continue;
      }
      if (fact.target && fact.target.revisionId !== detail.revision.revisionId)
        continue;
      annotations.push(annotationForFact(fact, content));
    }
    return annotations;
  }
  __name(annotationsForExactRevision, "annotationsForExactRevision");
  function bindCapturedDiffInteractions(diff, rendered) {
    rendered.ctx.files.forEach((file, index) => {
      const section = diff.querySelector(`[data-dfile="${index}"]`);
      const header = section?.querySelector(".dfile-head");
      const body = section?.querySelector(
        `[data-dfile-body="${index}"]`
      );
      const renderBody = /* @__PURE__ */ __name(() => {
        if (!section || !body) return;
        body.innerHTML = renderDiffFileBody(file, rendered.ctx.anchored);
        body.dataset.rendered = "1";
        section.dataset.expanded = "true";
      }, "renderBody");
      const toggle = /* @__PURE__ */ __name(() => {
        if (!section || !body) return;
        if (section.dataset.expanded === "true") {
          section.dataset.expanded = "false";
          return;
        }
        renderBody();
      }, "toggle");
      header?.addEventListener("click", toggle);
      header?.addEventListener("keydown", (event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        toggle();
      });
      body?.addEventListener("click", (event) => {
        const trigger = event.target?.closest(
          "[data-render-diff-file]"
        );
        if (!trigger) return;
        event.preventDefault();
        renderBody();
      });
    });
  }
  __name(bindCapturedDiffInteractions, "bindCapturedDiffInteractions");
  function renderCapturedDiff(resource, annotations = []) {
    const artifact = capturedDiffArtifact(resource.capturedDocument);
    if (artifact === null) {
      const refusal = document.createElement("section");
      refusal.className = "detail-facts";
      refusal.append(
        message(
          "This exact resource does not contain a captured snapshot. The Inspector will not reconstruct a diff from Git or an associated commit."
        )
      );
      return refusal;
    }
    const diff = document.createElement("section");
    diff.className = "captured-diff";
    const rendered = renderDiff(
      resource.resource.objectId,
      artifact,
      annotations
    );
    diff.innerHTML = rendered.html;
    rendered.ctx.files.forEach((file, index) => {
      const section = diff.querySelector(`[data-dfile="${index}"]`);
      const path = file.new_path ?? file.old_path;
      if (section && path) {
        section.dataset.filePath = path;
        if (file.old_path) section.dataset.oldFilePath = file.old_path;
        if (file.new_path) section.dataset.newFilePath = file.new_path;
        section.tabIndex = -1;
      }
    });
    bindCapturedDiffInteractions(diff, rendered);
    return diff;
  }
  __name(renderCapturedDiff, "renderCapturedDiff");
  function renderCapturedResource(resource, route, actions2) {
    const nodes = [
      detailHeading("Authoritative captured diff"),
      detailLine(shortExact2(resource.resource.revision), "mono"),
      detailLine(`availability: ${resource.availability.replaceAll("_", " ")}`)
    ];
    if (resource.availability !== "available") {
      nodes.push(
        message(
          "Captured bytes are unavailable. No live or associated-commit bytes were substituted."
        )
      );
    } else {
      nodes.push(
        detailLine(`captured document: ${resource.capturedDocumentHash}`, "mono")
      );
      nodes.push(renderCapturedDiff(resource));
    }
    for (const diagnostic of resource.diagnostics)
      nodes.push(detailLine(diagnostic));
    const back = document.createElement("button");
    back.type = "button";
    back.className = "ghost";
    back.textContent = "Back to exact Revision";
    back.addEventListener(
      "click",
      () => actions2.navigate({
        kind: "revision",
        changeId: route.changeId,
        revision: route.revision,
        query: route.query,
        ...route.focus ? { focus: route.focus } : {}
      })
    );
    nodes.push(back);
    return nodes;
  }
  __name(renderCapturedResource, "renderCapturedResource");
  function renderCurrentRevisionChoices(changeId, revisions, query, actions2) {
    const choices = document.createElement("section");
    choices.className = "detail-current-revisions";
    choices.append(detailHeading("Current Revisions", 3));
    if (revisions.length === 0) {
      choices.append(message("No current Revision is available."));
      return choices;
    }
    for (const revision of revisions) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "ghost mono";
      button.textContent = revision.revisionId;
      button.setAttribute(
        "aria-label",
        `Current Revision: open ${exactRevisionAccessibleIdentity(revision)}; for Change ${changeId}`
      );
      button.addEventListener(
        "click",
        () => actions2.navigate({
          kind: "revision",
          changeId,
          revision,
          query
        })
      );
      choices.append(button);
    }
    return choices;
  }
  __name(renderCurrentRevisionChoices, "renderCurrentRevisionChoices");
  function renderChangeDetail(detail, route, actions2) {
    const nodes = [
      detailHeading("Change"),
      detailLine(detail.summary.changeId, "mono"),
      detailLine(
        `declaration: ${detail.summary.declarationState.replaceAll("_", " ")} · topology: ${detail.summary.topology.replaceAll("_", " ")} · lifecycle: ${detail.summary.lifecycle.replaceAll("_", " ")}`
      ),
      detailLine(
        `members: ${detail.summary.memberCount} · current peers: ${detail.currentRevisionRefs.map(shortExact2).join("; ") || "none"}`
      ),
      renderCurrentRevisionChoices(
        route.changeId,
        detail.currentRevisionRefs,
        route.query,
        actions2
      )
    ];
    const sections = [
      [
        "Member Revisions",
        detail.memberRevisions.map(
          (member) => `${shortExact2(member.revision)} · support: ${member.supportingClaimIds.join(", ") || "none"}`
        )
      ],
      [
        "Unavailable Members",
        detail.unavailableMemberRevisions.map(
          (member) => `${member.revisionId} · ${member.reason.replaceAll("_", " ")} · support: ${member.supportingClaimIds.join(", ") || "none"}`
        )
      ],
      [
        "Membership Claims",
        detail.membershipClaims.map(
          (claim) => `${claim.claimId} · revision: ${claim.revisionId} · ${claim.active ? "active" : "inactive"} · supports: ${claim.supports.length} · withdrawals: ${claim.withdrawals.length}`
        )
      ],
      [
        "Membership Withdrawals",
        detail.membershipWithdrawals.map(
          (withdrawal) => `${withdrawal.claimId} · support carriers: ${withdrawal.supports.length}`
        )
      ],
      [
        "Revision Relation Claims",
        detail.relationClaims.map(
          (claim) => `${claim.claimId} · ${shortExact2(claim.predecessor)} → ${shortExact2(claim.successor)} · ${claim.active ? "active" : "inactive"}`
        )
      ],
      [
        "Relation Withdrawals",
        detail.relationWithdrawals.map(
          (withdrawal) => `${withdrawal.claimId} · support carriers: ${withdrawal.supports.length}`
        )
      ],
      [
        "Effective Supersedes",
        detail.effectiveSupersedes.map(
          ([successor, predecessor]) => `${shortExact2(predecessor)} → ${shortExact2(successor)}`
        )
      ],
      [
        "Pending or Conflicting Edges",
        detail.pendingOrConflictingEdges.map(
          (claim) => `${claim.claimId} · ${shortExact2(claim.predecessor)} → ${shortExact2(claim.successor)} · ${claim.active ? "active" : "inactive"}`
        )
      ],
      [
        "Change Links",
        detail.links.map(
          (link) => `${link.leftChangeId} · ${link.relation.replaceAll("_", " ")} · ${link.rightChangeId}`
        )
      ],
      [
        "Current Revision Qualification",
        detail.perCurrentRevisionQualification.map(
          (qualification) => `${shortExact2(qualification.revision)} · ${qualification.qualified ? "qualified" : "not qualified"}`
        )
      ],
      ["Operative Obligations", detail.operativeObligations],
      ["Diagnostics", detail.diagnostics]
    ];
    for (const [title, entries] of sections) {
      const section = document.createElement("section");
      section.append(detailHeading(title, 3));
      if (entries.length === 0) section.append(message("None."));
      for (const entry of entries) section.append(detailLine(entry));
      nodes.push(section);
    }
    return nodes;
  }
  __name(renderChangeDetail, "renderChangeDetail");
  function renderReading(reading, snapshot2, actions2) {
    const route = snapshot2.route;
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "ghost";
    copy.textContent = "Copy link";
    copy.addEventListener("click", () => copyExact(location.href));
    if (reading.kind === "change" && route.kind === "change") {
      return [...renderChangeDetail(reading.document, route, actions2), copy];
    }
    if (reading.kind === "resource" && route.kind === "resource")
      return [...renderCapturedResource(reading.document, route, actions2), copy];
    if (reading.kind === "interdiff" && route.kind === "interdiff") {
      const nodes = [
        detailHeading("Ordered Revision interdiff"),
        detailLine(
          `${shortExact2(reading.document.interdiff.from)} → ${shortExact2(reading.document.interdiff.to)}`,
          "mono"
        ),
        detailLine(
          `availability: ${reading.document.availability.replaceAll("_", " ")} · algorithm: ${reading.document.interdiff.algorithmVersion}`
        ),
        detailLine("This is a comparison, not the authoritative captured diff.")
      ];
      if (reading.document.comparison !== void 0) {
        const comparison = document.createElement("pre");
        comparison.textContent = JSON.stringify(
          reading.document.comparison,
          null,
          2
        );
        nodes.push(comparison);
      }
      for (const diagnostic of reading.document.diagnostics)
        nodes.push(detailLine(diagnostic));
      for (const revision of [route.from, route.to]) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "ghost";
        button.textContent = `Open authoritative captured diff: ${revision.revisionId}`;
        button.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "resource",
            changeId: route.changeId,
            revision,
            query: route.query
          })
        );
        nodes.push(button);
      }
      nodes.push(copy);
      return nodes;
    }
    if ((reading.kind === "revision" || reading.kind === "association") && (route.kind === "revision" || route.kind === "association")) {
      const document2 = reading.document;
      const nodes = [
        detailHeading(
          reading.kind === "association" ? "Association comparisons" : "Exact Revision"
        ),
        detailLine(shortExact2(document2.revision), "mono"),
        detailLine(
          `currency: ${document2.revisionCurrency.replaceAll("_", " ")} · relation: ${document2.relationClassification}`
        ),
        detailLine(
          `captured resource: ${document2.availability.replaceAll("_", " ")}`
        )
      ];
      if (reading.kind === "revision") {
        nodes.push(
          detailHeading("Authoritative captured diff", 3),
          renderCapturedDiff(
            document2.exactRevisionDocument,
            annotationsForExactRevision(document2)
          ),
          renderFacts(reading, route, actions2),
          renderFactPorts(reading)
        );
      }
      nodes.push(
        renderAssociations(reading, route, actions2),
        openCapturedResource(route, actions2),
        copy
      );
      return nodes;
    }
    return [
      message("Reading surface no longer matches the selected exact route.")
    ];
  }
  __name(renderReading, "renderReading");
  function exactFocusTarget(detail, route) {
    if (route.kind !== "revision" && route.kind !== "resource" && route.kind !== "association" && route.kind !== "interdiff") {
      return null;
    }
    const focus = route.focus;
    if (!focus) return null;
    const targets = [];
    if (focus.factId) {
      const fact = Array.from(
        detail.querySelectorAll("[data-fact-id], [data-anno]")
      ).find(
        (element) => element.dataset.factId === focus.factId || element.dataset.anno === focus.factId
      );
      if (fact) targets.push(fact);
    }
    if (focus.filePath) {
      const file = Array.from(
        detail.querySelectorAll("[data-file-path]")
      ).find(
        (element) => element.dataset.filePath === focus.filePath || element.dataset.oldFilePath === focus.filePath || element.dataset.newFilePath === focus.filePath
      );
      if (file) targets.push(file);
    }
    for (const target of targets) {
      target.dataset.exactFocus = "true";
      target.setAttribute("aria-current", "true");
    }
    return targets[0] ?? null;
  }
  __name(exactFocusTarget, "exactFocusTarget");
  function applyExactFocus(detail, route) {
    const target = exactFocusTarget(detail, route);
    if (!target) return;
    if (target.dataset.dfile !== void 0 && target.dataset.expanded !== "true") {
      target.querySelector(".dfile-head")?.click();
    }
    target.scrollIntoView?.({ block: "center", behavior: "auto" });
    target.focus({ preventScroll: true });
  }
  __name(applyExactFocus, "applyExactFocus");
  function renderDetail(snapshot2, actions2, presentation) {
    const detail = document.querySelector("#detail-body");
    if (!detail) return;
    if (snapshot2.route.kind === "invalid") {
      replaceDetailWith(message(snapshot2.route.message));
      return;
    }
    if (snapshot2.diagnostic) {
      replaceDetailWith(message(snapshot2.diagnostic));
      return;
    }
    if (snapshot2.route.kind === "lens" || snapshot2.generation === null) {
      replaceDetailWith(message("Select a Change or exact Revision."));
      return;
    }
    if (presentation.refusal !== null) {
      replaceDetailWith(
        message(`Reader refused this exact surface: ${presentation.refusal}`)
      );
      return;
    }
    if (presentation.reading !== null) {
      const readingKey = `${formatChangeInspectorRoute(snapshot2.route)}:${presentation.reading.document.projectionStamp}`;
      if (detail.dataset.changeReadingKey !== readingKey) {
        detail.replaceChildren(
          ...renderReading(presentation.reading, snapshot2, actions2)
        );
        detail.dataset.changeReadingKey = readingKey;
        applyExactFocus(detail, snapshot2.route);
      }
      return;
    }
    const heading = document.createElement("h2");
    heading.textContent = snapshot2.route.kind === "change" ? "Change" : snapshot2.route.kind === "resource" ? "Captured resource" : snapshot2.route.kind === "association" ? "Association comparison" : snapshot2.route.kind === "interdiff" ? "Revision interdiff" : "Exact Revision";
    const identity = document.createElement("p");
    identity.className = "mono";
    identity.textContent = snapshot2.route.kind === "change" ? `Change ID: ${snapshot2.route.changeId}` : snapshot2.route.kind === "interdiff" ? `From: ${snapshot2.route.from.revisionId} · ${snapshot2.route.from.objectArtifactContentHash}
To: ${snapshot2.route.to.revisionId} · ${snapshot2.route.to.objectArtifactContentHash}` : `Revision ID: ${snapshot2.route.revision.revisionId} · artifact hash: ${snapshot2.route.revision.objectArtifactContentHash}`;
    const placeholder = message(
      snapshot2.route.kind === "change" ? "Select an explicit current Revision to inspect its exact context." : "Exact reading surface is loading."
    );
    const copyLink = document.createElement("button");
    copyLink.type = "button";
    copyLink.className = "ghost";
    copyLink.textContent = "Copy link";
    copyLink.addEventListener("click", () => copyExact(location.href));
    const peers = document.createElement("section");
    if (snapshot2.route.kind === "change" && snapshot2.selected !== null) {
      const changeRoute = snapshot2.route;
      peers.append(
        renderCurrentRevisionChoices(
          changeRoute.changeId,
          snapshot2.selected.currentRevisionRefs,
          changeRoute.query,
          actions2
        )
      );
    }
    replaceDetailWith(heading, identity, copyLink, placeholder, peers);
  }
  __name(renderDetail, "renderDetail");
  function renderChangeInspector(snapshot2, actions2, presentation = {
    reading: null,
    refusal: null
  }) {
    const master = document.querySelector("#master");
    if (!master) return;
    const routeDiagnostic = document.querySelector("#route-diagnostic");
    if (routeDiagnostic) {
      routeDiagnostic.textContent = snapshot2.diagnostic ?? "";
      routeDiagnostic.classList.toggle("hidden", snapshot2.diagnostic === null);
    }
    syncFilterChrome(snapshot2.route);
    clearError();
    if (snapshot2.route.kind === "invalid") {
      replaceMasterWith(message("Cannot open this Inspector link."));
      renderDetail(snapshot2, actions2, presentation);
      return;
    }
    const route = snapshot2.route;
    if (snapshot2.generation === null) {
      replaceMasterWith(message("Loading Change generation…"));
      renderDetail(snapshot2, actions2, presentation);
      return;
    }
    const lens = lensForRoute(route);
    const page = lens === "changes" ? snapshot2.generation.changes : snapshot2.generation.attention;
    const listKey = JSON.stringify({
      lens,
      query: route.query,
      projectionStamp: page.projectionStamp,
      changes: page.changes.map((change) => change.changeId)
    });
    if (master.dataset.changeListKey !== listKey) {
      const list = document.createElement("section");
      list.className = "units";
      const heading = document.createElement("h1");
      heading.textContent = `${lens === "changes" ? "Changes" : "Attention"} · ${page.changes.length}`;
      list.append(heading);
      for (const summary of page.changes.slice(0, 150)) {
        const card = changeCardPresentation(
          summary,
          page.presentations?.[summary.changeId]
        );
        const element = document.createElement("article");
        element.className = "unit-card";
        element.dataset.changeId = summary.changeId;
        element.setAttribute("aria-label", card.accessibleName);
        const badges = document.createElement("p");
        badges.className = "change-card-badges";
        for (const value of card.badges) {
          const badge = document.createElement("span");
          badge.className = "badge";
          badge.textContent = value;
          badges.append(badge, " ");
        }
        element.append(badges);
        if (card.peers.length === 0) {
          const unavailable = document.createElement("h3");
          unavailable.textContent = "Current Revision unavailable";
          element.append(unavailable);
        } else if (card.peers.length > 1) {
          const peerHeading = document.createElement("h3");
          peerHeading.textContent = "Current Revisions";
          element.append(peerHeading);
        }
        for (const peer of card.peers) {
          const peerRow = document.createElement("div");
          peerRow.className = "change-card-peer";
          const choose = document.createElement("button");
          choose.type = "button";
          choose.className = "ghost change-card-peer-open";
          choose.textContent = peer.label;
          choose.title = peer.copyText;
          choose.setAttribute(
            "aria-label",
            `${peer.accessibleName}; open for Change ${summary.changeId}`
          );
          choose.addEventListener(
            "click",
            () => actions2.navigate({
              kind: "revision",
              changeId: summary.changeId,
              revision: peer.revision,
              query: queryForExactNavigation(route)
            })
          );
          const copyPeer = document.createElement("button");
          copyPeer.type = "button";
          copyPeer.className = "ghost";
          copyPeer.textContent = "Copy exact Revision";
          copyPeer.setAttribute(
            "aria-label",
            `Copy ${exactRevisionAccessibleIdentity(peer.revision)}; for Change ${summary.changeId}`
          );
          copyPeer.addEventListener("click", () => copyExact(peer.copyText));
          peerRow.append(choose, copyPeer);
          element.append(peerRow);
        }
        const actionsElement = document.createElement("div");
        actionsElement.className = "actions change-card-actions";
        const open = document.createElement("button");
        open.type = "button";
        open.className = "ghost";
        open.textContent = "Open Change";
        open.setAttribute("aria-label", `Open Change ${summary.changeId}`);
        open.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "change",
            changeId: summary.changeId,
            query: queryForExactNavigation(route)
          })
        );
        const changeIdentity = document.createElement("code");
        changeIdentity.className = "mono";
        changeIdentity.textContent = summary.changeId;
        const copyChange = document.createElement("button");
        copyChange.type = "button";
        copyChange.className = "ghost";
        copyChange.textContent = "Copy Change ID";
        copyChange.addEventListener("click", () => copyExact(summary.changeId));
        actionsElement.append(open, changeIdentity, copyChange);
        element.append(actionsElement);
        list.append(element);
      }
      if (page.changes.length === 0)
        list.append(
          message(
            lens === "changes" ? "No Changes." : "No Changes need attention."
          )
        );
      const nextPage = page.next;
      if (nextPage !== null) {
        const next = document.createElement("button");
        next.type = "button";
        next.className = "ghost";
        next.textContent = "Next page";
        next.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "lens",
            lens,
            query: {
              ...route.query,
              after: nextPage
            }
          })
        );
        list.append(next);
      }
      master.replaceChildren(list);
      master.dataset.changeListKey = listKey;
    }
    document.querySelectorAll("#lens-switcher [data-lens]").forEach((button) => {
      button.setAttribute("aria-pressed", String(button.dataset.lens === lens));
    });
    setText(
      "#stat-events",
      `${snapshot2.generation.profile.authorityCursor.eventCount ?? "—"} events`
    );
    setText(
      "#stat-units",
      `${snapshot2.generation.changes.changes.length} Changes`
    );
    setText(
      "#stat-threads",
      `${snapshot2.generation.attention.changes.length} need attention`
    );
    setText("#stat-hash", snapshot2.generation.changes.projectionStamp);
    renderDetail(snapshot2, actions2, presentation);
  }
  __name(renderChangeInspector, "renderChangeInspector");
  function renderChangeInspectorUnavailable(availability) {
    clearError();
    replaceMasterWith(
      message(
        availability === "migration_required" ? "Store migration required. No Change state was loaded." : "Store migration in progress. Partial Change state is unavailable."
      )
    );
    replaceDetailWith(message("Change state is unavailable."));
  }
  __name(renderChangeInspectorUnavailable, "renderChangeInspectorUnavailable");
  function renderChangeInspectorRefusal(error) {
    const text = error instanceof Error ? error.message : String(error);
    replaceMasterWith(message(`Reader refused: ${text}`));
    replaceDetailWith(message("Change state was not published."));
    const diagnostic = document.querySelector("#route-diagnostic");
    if (diagnostic) {
      diagnostic.textContent = "";
      diagnostic.classList.add("hidden");
    }
    const banner = document.querySelector("#error");
    if (banner) {
      banner.textContent = `error: ${text}`;
      banner.classList.remove("hidden");
    }
  }
  __name(renderChangeInspectorRefusal, "renderChangeInspectorRefusal");

  // src/change-inspector-state.ts
  var ChangeInspectorGenerationChanged = class extends Error {
    static {
      __name(this, "ChangeInspectorGenerationChanged");
    }
    constructor() {
      super("Change generation changed during staging");
    }
  };
  function sameRevision2(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameRevision2, "sameRevision");
  function selectedChange(generation, route) {
    if (route.kind !== "change" && route.kind !== "revision") return null;
    const all = [...generation.changes.changes, ...generation.attention.changes];
    const change = all.find((candidate) => candidate.changeId === route.changeId) ?? null;
    if (change === null || route.kind !== "revision") return change;
    return change.currentRevisionRefs.some(
      (candidate) => sameRevision2(candidate, route.revision)
    ) ? change : null;
  }
  __name(selectedChange, "selectedChange");
  function selectionDiagnostic(route) {
    if (route.kind === "invalid") return route.message;
    return null;
  }
  __name(selectionDiagnostic, "selectionDiagnostic");
  function stageGeneration(profile, changes, attention, postflight) {
    requireCoherentGeneration(changes, attention);
    if (!sameProfileGeneration(profile, postflight)) {
      throw new ChangeInspectorGenerationChanged();
    }
    return { profile, changes, attention };
  }
  __name(stageGeneration, "stageGeneration");
  function createChangeInspectorState(initialRoute) {
    let generation = null;
    let route = initialRoute;
    const snapshot2 = /* @__PURE__ */ __name(() => {
      const selected = generation === null ? null : selectedChange(generation, route);
      return {
        generation,
        route,
        selected,
        diagnostic: selectionDiagnostic(route)
      };
    }, "snapshot");
    return {
      snapshot: snapshot2,
      publish(next) {
        generation = next;
        return snapshot2();
      },
      setRoute(next) {
        route = next;
        return snapshot2();
      },
      clearGeneration() {
        generation = null;
        return snapshot2();
      }
    };
  }
  __name(createChangeInspectorState, "createChangeInspectorState");

  // src/disclosure.ts
  var active = null;
  function createDisclosure({
    container,
    trigger,
    panel
  }) {
    let open = false;
    const triggerElement = $(trigger);
    const containerElement = $(container);
    let controller;
    const onTriggerClick = /* @__PURE__ */ __name((event) => {
      event.stopPropagation();
      controller.toggle();
    }, "onTriggerClick");
    const onContainerKeydown = /* @__PURE__ */ __name((event) => {
      if (event.key !== "Escape" || !open) return;
      event.preventDefault();
      event.stopPropagation();
      controller.close(true);
    }, "onContainerKeydown");
    const onDocumentClick = /* @__PURE__ */ __name((event) => {
      if (!open) return;
      const root = $(container);
      if (event.target instanceof Node && root?.contains(event.target)) return;
      controller.close();
    }, "onDocumentClick");
    controller = {
      isOpen: /* @__PURE__ */ __name(() => open, "isOpen"),
      open: /* @__PURE__ */ __name(() => {
        if (active && active !== controller) active.close();
        open = true;
        active = controller;
        controller.sync();
      }, "open"),
      close: /* @__PURE__ */ __name((returnFocus = false) => {
        open = false;
        if (active === controller) active = null;
        controller.sync();
        if (returnFocus) $(trigger)?.focus();
      }, "close"),
      toggle: /* @__PURE__ */ __name(() => {
        if (open) controller.close();
        else controller.open();
      }, "toggle"),
      sync: /* @__PURE__ */ __name(() => {
        $(panel)?.classList.toggle("hidden", !open);
        $(trigger)?.setAttribute("aria-expanded", String(open));
      }, "sync"),
      dispose: /* @__PURE__ */ __name(() => {
        triggerElement?.removeEventListener("click", onTriggerClick);
        containerElement?.removeEventListener("keydown", onContainerKeydown);
        document.removeEventListener("click", onDocumentClick, true);
        if (active === controller) active = null;
        open = false;
        controller.sync();
      }, "dispose")
    };
    triggerElement?.addEventListener("click", onTriggerClick);
    containerElement?.addEventListener("keydown", onContainerKeydown);
    document.addEventListener("click", onDocumentClick, true);
    controller.sync();
    return controller;
  }
  __name(createDisclosure, "createDisclosure");

  // src/change-inspector.ts
  var EXACT_READING_TIMEOUT_MS = 1e4;
  var POLL_CYCLE_TIMEOUT_MS = 15e3;
  var ChangeInspectorTimeout = class extends Error {
    static {
      __name(this, "ChangeInspectorTimeout");
    }
  };
  var pollTimer = null;
  var routeListener = null;
  var filterInput = null;
  var filterInputListener = null;
  var connectionControlsInitialized = false;
  var filterDisclosure = null;
  var viewDisclosure = null;
  var interactionStop = null;
  var pollCoordinatorStop = null;
  var requestEpoch = 0;
  function currentRoute() {
    return parseChangeInspectorRoute(location.hash || "#/changes");
  }
  __name(currentRoute, "currentRoute");
  function newProjectionRetryBudget() {
    return { remaining: 1 };
  }
  __name(newProjectionRetryBudget, "newProjectionRetryBudget");
  function consumeProjectionRetry(budget) {
    if (budget.remaining === 0) return false;
    budget.remaining -= 1;
    return true;
  }
  __name(consumeProjectionRetry, "consumeProjectionRetry");
  function snapshotFilterDraft(input, restoreFocus) {
    return {
      input,
      restoreFocus,
      value: input.value,
      selectionStart: input.selectionStart,
      selectionEnd: input.selectionEnd,
      selectionDirection: input.selectionDirection
    };
  }
  __name(snapshotFilterDraft, "snapshotFilterDraft");
  function capturePollFilterDraft() {
    if (filterInput === null) return null;
    const route = currentRoute();
    const committed = route.kind === "invalid" ? "" : route.query.q ?? "";
    const focused = document.activeElement === filterInput;
    if (!focused && filterInput.value === committed) return null;
    return snapshotFilterDraft(filterInput, focused);
  }
  __name(capturePollFilterDraft, "capturePollFilterDraft");
  async function withinTimeout(operation, timeoutMs, message2) {
    let timer = null;
    try {
      return await Promise.race([
        operation,
        new Promise((_resolve, reject) => {
          timer = setTimeout(
            () => reject(new ChangeInspectorTimeout(message2)),
            timeoutMs
          );
        })
      ]);
    } finally {
      if (timer !== null) clearTimeout(timer);
    }
  }
  __name(withinTimeout, "withinTimeout");
  function stopChangeInspector() {
    requestEpoch += 1;
    if (pollTimer !== null) clearInterval(pollTimer);
    pollTimer = null;
    pollCoordinatorStop?.();
    pollCoordinatorStop = null;
    if (routeListener !== null)
      window.removeEventListener("hashchange", routeListener);
    routeListener = null;
    if (filterInput !== null && filterInputListener !== null) {
      filterInput.removeEventListener("change", filterInputListener);
    }
    filterInput = null;
    filterInputListener = null;
    filterDisclosure?.dispose();
    filterDisclosure = null;
    viewDisclosure?.dispose();
    viewDisclosure = null;
    interactionStop?.();
    interactionStop = null;
  }
  __name(stopChangeInspector, "stopChangeInspector");
  async function bootstrapChangeInspector(options = {}) {
    stopChangeInspector();
    const capability = bootstrapCapability();
    if (capability.token !== null) {
      (options.reload ?? (() => location.reload()))();
      return;
    }
    installDefaultAuthCoordinator();
    const state = createChangeInspectorState(currentRoute());
    const navigate = /* @__PURE__ */ __name((route) => {
      const hash = formatChangeInspectorRoute(route);
      if (location.hash !== hash) location.hash = hash;
      else void onRoute();
    }, "navigate");
    let reading = null;
    let readingRefusal = null;
    let visibleReading = "";
    const paint = /* @__PURE__ */ __name((pollDraft = null) => {
      const draft = pollDraft !== null && filterInput === pollDraft.input ? snapshotFilterDraft(
        filterInput,
        document.activeElement === filterInput || document.activeElement === document.body && pollDraft.restoreFocus
      ) : null;
      renderChangeInspector(
        state.snapshot(),
        { navigate },
        {
          reading,
          refusal: readingRefusal
        }
      );
      if (draft !== null && filterInput !== null) {
        filterInput.value = draft.value;
        if (draft.restoreFocus) filterInput.focus({ preventScroll: true });
        if (draft.selectionStart !== null && draft.selectionEnd !== null) {
          filterInput.setSelectionRange(
            draft.selectionStart,
            draft.selectionEnd,
            draft.selectionDirection ?? void 0
          );
        }
      }
      interaction?.sync(state.snapshot());
    }, "paint");
    let interaction = null;
    const requestKey = /* @__PURE__ */ __name((query) => buildChangePageUrl("changes", query), "requestKey");
    let visibleRequest = "";
    let pendingReading = null;
    let releaseQueuedPoll = /* @__PURE__ */ __name(() => {
    }, "releaseQueuedPoll");
    const readingKey = /* @__PURE__ */ __name((route, projectionStamp) => `${formatChangeInspectorRoute(route)}\0${projectionStamp}`, "readingKey");
    const clearReading = /* @__PURE__ */ __name(() => {
      reading = null;
      readingRefusal = null;
      visibleReading = "";
    }, "clearReading");
    const loadReading = /* @__PURE__ */ __name(async (route, expectedProjectionStamp, epoch, retryBudget, pollDraft = null) => {
      if (route.kind === "lens") {
        clearReading();
        return;
      }
      const requested = formatChangeInspectorRoute(route);
      const requestedReading = readingKey(route, expectedProjectionStamp);
      if (visibleReading === requestedReading && reading !== null) return;
      reading = null;
      readingRefusal = null;
      visibleReading = requestedReading;
      paint(pollDraft);
      const pendingToken = /* @__PURE__ */ Symbol("exact-reading");
      pendingReading = { key: requestedReading, token: pendingToken };
      try {
        const { loaded, postflight } = await withinTimeout(
          (async () => {
            const loaded2 = await loadChangeInspectorReading(
              route,
              expectedProjectionStamp
            );
            const postflight2 = decodeReaderProfile(
              await fetchChangeInspectorJSON("/api/v2/profile")
            );
            return { loaded: loaded2, postflight: postflight2 };
          })(),
          EXACT_READING_TIMEOUT_MS,
          "exact Change reading timed out"
        );
        if (epoch !== requestEpoch || currentRoute().kind === "invalid") return;
        const staged = state.snapshot().generation;
        if (staged === null || formatChangeInspectorRoute(
          currentRoute()
        ) !== requested || !sameProfileGeneration(staged.profile, postflight)) {
          throw new ChangeInspectorGenerationChanged();
        }
        reading = loaded;
        readingRefusal = null;
        paint(pollDraft);
      } catch (error) {
        if (epoch !== requestEpoch) return;
        if ((error instanceof ChangeInspectorGenerationChanged || error instanceof ChangeInspectorPageFailure && error.code === "stale_projection") && consumeProjectionRetry(retryBudget)) {
          await loadGeneration(route, retryBudget, pollDraft);
          return;
        }
        reading = null;
        readingRefusal = error instanceof Error ? error.message : String(error);
        paint(pollDraft);
      } finally {
        if (pendingReading?.token === pendingToken) pendingReading = null;
        releaseQueuedPoll();
      }
    }, "loadReading");
    const loadGeneration = /* @__PURE__ */ __name(async (route, retryBudget, pollDraft = null) => {
      const epoch = ++requestEpoch;
      try {
        const profile = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile")
        );
        if (epoch !== requestEpoch) return;
        if (profile.availability !== "ready") {
          visibleRequest = "";
          clearReading();
          state.clearGeneration();
          renderChangeInspectorUnavailable(profile.availability);
          return;
        }
        const query = route.query;
        const activeLens = lensForRoute(route);
        const changesQuery = activeLens === "changes" ? query : firstPageQuery(query);
        const attentionQuery = activeLens === "attention" ? query : firstPageQuery(query);
        const [changes, attention] = await Promise.all([
          fetchChangeInspectorJSON(
            buildChangePageUrl("changes", changesQuery)
          ).then(
            (value) => decodeChangePage(value, { lens: "changes", bounded: true })
          ),
          fetchChangeInspectorJSON(
            buildChangePageUrl("attention", attentionQuery)
          ).then(
            (value) => decodeChangePage(value, { lens: "attention", bounded: true })
          )
        ]);
        const postflight = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile")
        );
        if (epoch !== requestEpoch) return;
        const staged = stageGeneration(profile, changes, attention, postflight);
        if (route.kind !== "lens") {
          const requestedReading = readingKey(route, changes.projectionStamp);
          if (visibleReading !== requestedReading) {
            reading = null;
            readingRefusal = null;
          }
        }
        state.publish(staged);
        visibleRequest = requestKey(query);
        paint(pollDraft);
        await loadReading(
          route,
          changes.projectionStamp,
          epoch,
          retryBudget,
          pollDraft
        );
      } catch (error) {
        if (epoch !== requestEpoch) return;
        if ((error instanceof ChangeInspectorPageFailure && error.code === "stale_projection" || error instanceof ChangeInspectorGenerationChanged) && consumeProjectionRetry(retryBudget)) {
          await loadGeneration(route, retryBudget, pollDraft);
          return;
        }
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
      }
    }, "loadGeneration");
    const onRoute = /* @__PURE__ */ __name(async () => {
      const route = currentRoute();
      requestEpoch += 1;
      state.setRoute(route);
      if (route.kind === "invalid") {
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        paint();
        return;
      }
      let request;
      try {
        request = requestKey(route.query);
      } catch (error) {
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
        return;
      }
      if (request === visibleRequest) {
        const generation = state.snapshot().generation;
        if (generation === null) {
          await loadGeneration(route, newProjectionRetryBudget());
        } else {
          await loadReading(
            route,
            generation.changes.projectionStamp,
            requestEpoch,
            newProjectionRetryBudget()
          );
          paint();
        }
      } else {
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        paint();
        await loadGeneration(route, newProjectionRetryBudget());
      }
    }, "onRoute");
    routeListener = /* @__PURE__ */ __name(() => {
      void onRoute();
    }, "routeListener");
    window.addEventListener("hashchange", routeListener);
    const reloadCurrent = /* @__PURE__ */ __name(async () => {
      const route = currentRoute();
      if (route.kind === "invalid") {
        await onRoute();
        return;
      }
      await loadGeneration(route, newProjectionRetryBudget());
    }, "reloadCurrent");
    configureConnectionActions({
      retry: reloadCurrent,
      reconnect: /* @__PURE__ */ __name(async () => {
        if (await requestReconnect()) await reloadCurrent();
      }, "reconnect")
    });
    if (!connectionControlsInitialized) {
      initConnectionControls();
      connectionControlsInitialized = true;
    }
    prepareChangeInspectorShell({ navigate });
    filterDisclosure = createDisclosure({
      container: "#filter-controls",
      trigger: "#filters-toggle",
      panel: "#filters-panel"
    });
    viewDisclosure = createDisclosure({
      container: "#view-controls",
      trigger: "#view-toggle",
      panel: "#view-panel"
    });
    interaction = installChangeInspectorInteraction({ navigate });
    interactionStop = interaction.stop;
    filterInput = document.querySelector("#filter-text");
    filterInputListener = /* @__PURE__ */ __name(() => {
      const route = currentRoute();
      const base = route.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : route;
      navigate({
        ...base,
        query: {
          ...base.query,
          after: void 0,
          q: filterInput?.value || void 0
        }
      });
    }, "filterInputListener");
    filterInput?.addEventListener("change", filterInputListener);
    await onRoute();
    if (options.poll !== false) {
      let pollRequested = false;
      let pollRunning = false;
      let pollActive = true;
      const drainPoll = /* @__PURE__ */ __name(() => {
        if (!pollActive || pollRunning || !pollRequested) return;
        const route = currentRoute();
        if (route.kind === "invalid") {
          pollRequested = false;
          return;
        }
        const generation = state.snapshot().generation;
        if (route.kind !== "lens" && generation !== null && pendingReading?.key === readingKey(route, generation.changes.projectionStamp)) {
          return;
        }
        pollRequested = false;
        pollRunning = true;
        const operation = loadGeneration(
          route,
          newProjectionRetryBudget(),
          capturePollFilterDraft()
        );
        void withinTimeout(
          operation,
          POLL_CYCLE_TIMEOUT_MS,
          "Change generation poll timed out"
        ).catch((error) => {
          if (error instanceof ChangeInspectorTimeout) {
            requestEpoch += 1;
          }
        }).finally(() => {
          pollRunning = false;
          drainPoll();
        });
      }, "drainPoll");
      const requestPoll = /* @__PURE__ */ __name(() => {
        pollRequested = true;
        drainPoll();
      }, "requestPoll");
      releaseQueuedPoll = drainPoll;
      pollCoordinatorStop = /* @__PURE__ */ __name(() => {
        pollActive = false;
        pollRequested = false;
      }, "pollCoordinatorStop");
      pollTimer = setInterval(requestPoll, 3e3);
    }
  }
  __name(bootstrapChangeInspector, "bootstrapChangeInspector");

  // src/entry.ts
  void bootstrapChangeInspector();
})();
