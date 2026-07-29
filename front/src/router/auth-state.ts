import type { AuthLifecycle, BootstrapLoadState } from "@/store/authStore";

export type AuthRouteKind = "bootstrap" | "login" | "protected" | "public";
export type AuthRouteDecision = "allow" | "bootstrap" | "login" | "dashboard" | "restore";

export interface AuthRouteDecisionInput {
  bootstrapState: BootstrapLoadState;
  routeKind: AuthRouteKind;
  lifecycle: AuthLifecycle;
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
    if (input.lifecycle === "authenticated") return "dashboard";
    return input.lifecycle === "anonymous" ? "login" : "restore";
  }
  if (input.routeKind === "protected") {
    if (input.lifecycle === "authenticated") return "allow";
    return input.lifecycle === "anonymous" ? "login" : "restore";
  }
  if (input.routeKind === "login") {
    if (input.lifecycle === "authenticated") return "dashboard";
    return input.lifecycle === "anonymous" ? "allow" : "restore";
  }
  return "allow";
}
