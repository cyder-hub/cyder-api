export const MANAGER_AUTH_CHANNEL_NAME = "cyder-manager-auth-v1";
export const MANAGER_AUTH_EVENT_SCHEMA_VERSION = 1;

export type ManagerAuthCoordinationEvent =
  | { schema_version: 1; type: "session_changed" }
  | { schema_version: 1; type: "session_revoked" };

interface MessageEventLike {
  data: unknown;
}

export interface BroadcastChannelLike {
  addEventListener: (
    type: "message",
    listener: (event: MessageEventLike) => void,
  ) => void;
  removeEventListener: (
    type: "message",
    listener: (event: MessageEventLike) => void,
  ) => void;
  postMessage: (event: ManagerAuthCoordinationEvent) => void;
  close: () => void;
}

interface WindowLifecycleTarget {
  addEventListener: (type: "focus", listener: () => void) => void;
  removeEventListener: (type: "focus", listener: () => void) => void;
}

interface DocumentLifecycleTarget {
  visibilityState: string;
  addEventListener: (
    type: "visibilitychange",
    listener: () => void,
  ) => void;
  removeEventListener: (
    type: "visibilitychange",
    listener: () => void,
  ) => void;
}

export interface AuthCoordinationDependencies {
  recoverAccess: () => Promise<string>;
  invalidateAccessRecovery: () => void;
  revokeLocalSession: () => void;
}

export interface AuthCoordinationEnvironment {
  createChannel?: () => BroadcastChannelLike | null;
  windowTarget?: WindowLifecycleTarget | null;
  documentTarget?: DocumentLifecycleTarget | null;
}

function defaultChannel(): BroadcastChannelLike | null {
  if (typeof BroadcastChannel === "undefined") return null;
  return new BroadcastChannel(
    MANAGER_AUTH_CHANNEL_NAME,
  ) as BroadcastChannelLike;
}

function isCoordinationEvent(
  value: unknown,
): value is ManagerAuthCoordinationEvent {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const event = value as Record<string, unknown>;
  const keys = Object.keys(event).sort();
  return (
    keys.length === 2 &&
    keys[0] === "schema_version" &&
    keys[1] === "type" &&
    event.schema_version === MANAGER_AUTH_EVENT_SCHEMA_VERSION &&
    (event.type === "session_changed" || event.type === "session_revoked")
  );
}

export function createAuthCoordination(
  deps: AuthCoordinationDependencies,
  environment: AuthCoordinationEnvironment = {},
) {
  let channel: BroadcastChannelLike | null = null;
  try {
    channel = (environment.createChannel ?? defaultChannel)();
  } catch {
    channel = null;
  }

  const windowTarget =
    environment.windowTarget === undefined
      ? typeof window === "undefined"
        ? null
        : window
      : environment.windowTarget;
  const documentTarget =
    environment.documentTarget === undefined
      ? typeof document === "undefined"
        ? null
        : document
      : environment.documentTarget;

  const converge = (): void => {
    if (documentTarget?.visibilityState === "hidden") return;
    void deps.recoverAccess().catch(() => {
      // Auth session recovery owns lifecycle classification.
    });
  };

  const onMessage = (message: MessageEventLike): void => {
    if (!isCoordinationEvent(message.data)) return;
    if (message.data.type === "session_revoked") {
      deps.revokeLocalSession();
      return;
    }
    deps.invalidateAccessRecovery();
    converge();
  };
  const onFocus = (): void => converge();
  const onVisibilityChange = (): void => {
    if (documentTarget?.visibilityState === "visible") converge();
  };

  channel?.addEventListener("message", onMessage);
  windowTarget?.addEventListener("focus", onFocus);
  documentTarget?.addEventListener("visibilitychange", onVisibilityChange);

  const announce = (type: ManagerAuthCoordinationEvent["type"]): void => {
    channel?.postMessage({
      schema_version: MANAGER_AUTH_EVENT_SCHEMA_VERSION,
      type,
    });
  };

  return {
    announceSessionChanged: () => announce("session_changed"),
    announceSessionRevoked: () => announce("session_revoked"),
    dispose: () => {
      channel?.removeEventListener("message", onMessage);
      channel?.close();
      windowTarget?.removeEventListener("focus", onFocus);
      documentTarget?.removeEventListener(
        "visibilitychange",
        onVisibilityChange,
      );
    },
  };
}
