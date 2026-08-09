export type ConnectionState =
  | "connecting"
  | "connected"
  | "unauthorized"
  | "unreachable";
export type RefreshState = "idle" | "watching" | "updated" | "degraded";
export type RequestFailureKind = "unauthorized" | "unreachable" | "protocol";

export interface ConnectionSnapshot {
  connection: ConnectionState;
  refresh: RefreshState;
}

export interface ConnectionPresentation {
  serverLabel: string;
  connectionLabel: string;
  refreshLabel: string;
  action: "Reconnect" | "Retry" | null;
  canConnectAnother: boolean;
}

let snapshot: ConnectionSnapshot = {
  connection: "connecting",
  refresh: "idle",
};

export function getConnectionSnapshot(): ConnectionSnapshot {
  return { ...snapshot };
}

export function connectionPresentation(
  state: ConnectionSnapshot,
): ConnectionPresentation {
  const refreshLabel =
    state.refresh === "degraded" ? "response error" : state.refresh;
  switch (state.connection) {
    case "unauthorized":
      return {
        serverLabel: "local server",
        connectionLabel: "authentication required",
        refreshLabel,
        action: "Reconnect",
        canConnectAnother: false,
      };
    case "unreachable":
      return {
        serverLabel: "local server",
        connectionLabel: "server unavailable",
        refreshLabel,
        action: "Retry",
        canConnectAnother: true,
      };
    case "connected":
      return {
        serverLabel: "local server",
        connectionLabel: "connected",
        refreshLabel,
        action: state.refresh === "degraded" ? "Retry" : null,
        canConnectAnother: false,
      };
    case "connecting":
      return {
        serverLabel: "local server",
        connectionLabel: "connecting",
        refreshLabel,
        action: null,
        canConnectAnother: false,
      };
  }
}

export function markRequestSuccess(): void {
  snapshot = {
    connection: "connected",
    refresh: snapshot.refresh,
  };
  renderConnectionChrome();
}

export function markRequestFailure(
  kind: RequestFailureKind,
  options: { degradeRefresh?: boolean } = {},
): void {
  snapshot =
    kind === "protocol"
      ? {
          connection: "connected",
          refresh:
            options.degradeRefresh === false ? snapshot.refresh : "degraded",
        }
      : { ...snapshot, connection: kind };
  renderConnectionChrome();
}

export function setRefreshState(refresh: RefreshState): void {
  snapshot = { ...snapshot, refresh };
  renderConnectionChrome();
}

export function resetConnectionState(): void {
  snapshot = { connection: "connecting", refresh: "idle" };
  actions = null;
}

interface ConnectionActions {
  retry: () => void | Promise<void>;
  reconnect: () => void | Promise<void>;
}

let actions: ConnectionActions | null = null;

export function configureConnectionActions(next: ConnectionActions): void {
  actions = next;
}

export function initConnectionControls(): void {
  document
    .querySelector("#connection-action")
    ?.addEventListener("click", () => {
      if (!actions) return;
      if (snapshot.connection === "unauthorized") void actions.reconnect();
      else void actions.retry();
    });
  document.querySelector("#connect-another")?.addEventListener("click", () => {
    if (actions) void actions.reconnect();
  });
  renderConnectionChrome();
}

export function renderConnectionChrome(): void {
  const presentation = connectionPresentation(snapshot);
  const root = document.querySelector<HTMLElement>("#store-identity");
  root?.classList.remove("hidden");
  const connection = document.querySelector<HTMLElement>("#connection-status");
  if (connection) connection.textContent = presentation.connectionLabel;
  const refresh = document.querySelector<HTMLElement>("#refresh-status");
  if (refresh) refresh.textContent = presentation.refreshLabel;
  const legacyRefresh = document.querySelector<HTMLElement>("#stat-live");
  if (legacyRefresh) {
    legacyRefresh.textContent = presentation.refreshLabel;
    legacyRefresh.dataset.state = snapshot.refresh;
  }
  const dot = document.querySelector<HTMLElement>("#refresh");
  if (dot) {
    dot.dataset.connection = snapshot.connection;
    dot.dataset.state = snapshot.refresh;
    dot.title = `${presentation.connectionLabel}; refresh ${presentation.refreshLabel}`;
  }
  const action =
    document.querySelector<HTMLButtonElement>("#connection-action");
  if (action) {
    action.textContent = presentation.action ?? "";
    action.classList.toggle("hidden", presentation.action === null);
  }
  document
    .querySelector<HTMLElement>("#connect-another")
    ?.classList.toggle("hidden", !presentation.canConnectAnother);
  const word = document.querySelector<HTMLElement>("#refresh-word");
  if (word) {
    word.textContent =
      snapshot.connection === "unauthorized"
        ? "authentication required"
        : snapshot.connection === "unreachable"
          ? "server unavailable"
          : snapshot.refresh === "degraded"
            ? "response error"
            : "";
  }
}
