// ========== Auth Types ==========
export interface User {
  username: string;
}

export interface ManagerAuthAccess {
  access_token: string;
}

export interface LogoutAllResult {
  revoked_sessions: number;
}

export type ManagerBootstrapState = "uninitialized" | "ready";

export interface ManagerBootstrapStatus {
  state: ManagerBootstrapState;
}
