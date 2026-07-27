import type { ManagerTotpState, ManagerTotpStatus } from "@/services/types";

export function effectiveManagerTotpState(
  storeState: ManagerTotpState | null,
  status: ManagerTotpStatus | null,
): ManagerTotpState {
  return storeState ?? status?.state ?? "unavailable";
}

export function reconcileManagerTotpStatus(
  storeState: ManagerTotpState | null,
  status: ManagerTotpStatus | null,
): ManagerTotpStatus | null {
  if (!storeState || status?.state === storeState) return status;
  return { state: storeState };
}
