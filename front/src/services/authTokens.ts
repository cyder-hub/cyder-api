import type { AuthTokenPair } from "./types";

export const AUTH_SESSION_STORAGE_KEY = "auth_token";
export const AUTH_SESSION_SCHEMA_VERSION = 1;

export interface AuthSessionRecord {
  schema_version: 1;
  revision: number;
  refresh_token: string;
  access_token: string;
}

export type StoredAuthSession =
  | { kind: "record"; record: AuthSessionRecord }
  | { kind: "legacy"; refresh_token: string };

type ReadableTokenStorage = Pick<Storage, "getItem">;
type WritableTokenStorage = Pick<Storage, "getItem" | "setItem">;
type RemovableTokenStorage = Pick<Storage, "removeItem">;
type SessionTokenStorage = Pick<Storage, "getItem" | "removeItem">;
type StorageEventTarget = Pick<Window, "addEventListener" | "removeEventListener">;
const localSessionSubscribers = new Set<
  (session: StoredAuthSession | null) => void
>();

function notifyLocalSessionChange(session: StoredAuthSession | null): void {
  localSessionSubscribers.forEach((subscriber) => subscriber(session));
}

function isAuthSessionRecord(value: unknown): value is AuthSessionRecord {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  if (
    keys.length !== 4 ||
    keys[0] !== "access_token" ||
    keys[1] !== "refresh_token" ||
    keys[2] !== "revision" ||
    keys[3] !== "schema_version"
  ) {
    return false;
  }
  return (
    record.schema_version === AUTH_SESSION_SCHEMA_VERSION &&
    Number.isSafeInteger(record.revision) &&
    (record.revision as number) >= 0 &&
    typeof record.refresh_token === "string" &&
    record.refresh_token.length > 0 &&
    typeof record.access_token === "string" &&
    record.access_token.length > 0
  );
}

export function readStoredAuthSession(
  storage: ReadableTokenStorage = localStorage,
): StoredAuthSession | null {
  const raw = storage.getItem(AUTH_SESSION_STORAGE_KEY);
  if (!raw) return null;

  try {
    const parsed: unknown = JSON.parse(raw);
    return isAuthSessionRecord(parsed) ? { kind: "record", record: parsed } : null;
  } catch {
    return { kind: "legacy", refresh_token: raw };
  }
}

export function readStoredSessionRecord(
  storage: ReadableTokenStorage = localStorage,
): AuthSessionRecord | null {
  const session = readStoredAuthSession(storage);
  return session?.kind === "record" ? session.record : null;
}

export function persistAuthTokenPair(
  tokenPair: AuthTokenPair,
  storage: WritableTokenStorage = localStorage,
  now: () => number = Date.now,
): AuthSessionRecord {
  const currentRevision = readStoredSessionRecord(storage)?.revision ?? -1;
  if (currentRevision >= Number.MAX_SAFE_INTEGER) {
    throw new Error("auth session revision exhausted");
  }
  const timestamp = Math.max(0, Math.trunc(now()));
  const revision = Math.max(timestamp, currentRevision + 1);
  if (!Number.isSafeInteger(revision)) {
    throw new Error("auth session revision is invalid");
  }
  const record: AuthSessionRecord = {
    schema_version: AUTH_SESSION_SCHEMA_VERSION,
    revision,
    refresh_token: tokenPair.refresh_token,
    access_token: tokenPair.access_token,
  };
  storage.setItem(AUTH_SESSION_STORAGE_KEY, JSON.stringify(record));
  notifyLocalSessionChange({ kind: "record", record });
  return record;
}

export function clearStoredAuthSession(
  storage: RemovableTokenStorage = localStorage,
): void {
  storage.removeItem(AUTH_SESSION_STORAGE_KEY);
  notifyLocalSessionChange(null);
}

export function clearStoredAuthSessionIfCurrent(
  refreshToken: string,
  storage: SessionTokenStorage = localStorage,
): boolean {
  const session = readStoredAuthSession(storage);
  const currentRefreshToken =
    session?.kind === "record" ? session.record.refresh_token : session?.refresh_token;
  if (currentRefreshToken !== refreshToken) return false;
  storage.removeItem(AUTH_SESSION_STORAGE_KEY);
  return true;
}

export function subscribeToAuthSessionChanges(
  onChange: (session: StoredAuthSession | null) => void,
  eventTarget: StorageEventTarget = window,
  storage: ReadableTokenStorage = localStorage,
): () => void {
  localSessionSubscribers.add(onChange);
  const unsubscribeStorage = subscribeToAuthSessionStorage(
    onChange,
    eventTarget,
    storage,
  );
  return () => {
    localSessionSubscribers.delete(onChange);
    unsubscribeStorage();
  };
}

export function subscribeToAuthSessionStorage(
  onChange: (session: StoredAuthSession | null) => void,
  eventTarget: StorageEventTarget = window,
  storage: ReadableTokenStorage = localStorage,
): () => void {
  const listener = (event: StorageEvent): void => {
    if (event.key !== AUTH_SESSION_STORAGE_KEY) return;
    onChange(readStoredAuthSession(storage));
  };
  eventTarget.addEventListener("storage", listener);
  return () => eventTarget.removeEventListener("storage", listener);
}
