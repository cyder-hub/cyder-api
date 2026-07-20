import type { BootstrapLoadState } from "@/store/authStore";

export type AuthRouteKind = "bootstrap" | "login" | "protected" | "public";
export type AuthRouteDecision = "allow" | "bootstrap" | "login" | "dashboard" | "restore";

export interface AuthRouteDecisionInput {
  bootstrapState: BootstrapLoadState;
  routeKind: AuthRouteKind;
  hasRefreshToken: boolean;
  hasAccessToken: boolean;
}

export function decideAuthRoute(input: AuthRouteDecisionInput): AuthRouteDecision {
  if (input.bootstrapState === "error") {
    return input.routeKind === "bootstrap" ? "allow" : "bootstrap";
  }
  if (input.bootstrapState === "uninitialized") {
    return input.routeKind === "bootstrap" ? "allow" : "bootstrap";
  }
  if (input.bootstrapState !== "ready") {
    return "bootstrap";
  }

  if (input.routeKind === "bootstrap") {
    if (input.hasAccessToken) return "dashboard";
    return input.hasRefreshToken ? "restore" : "login";
  }
  if (input.routeKind === "protected") {
    if (input.hasAccessToken) return "allow";
    return input.hasRefreshToken ? "restore" : "login";
  }
  if (input.routeKind === "login") {
    if (input.hasAccessToken) return "dashboard";
    return input.hasRefreshToken ? "restore" : "allow";
  }
  return "allow";
}
