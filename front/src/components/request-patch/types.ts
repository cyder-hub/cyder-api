import type {
  RequestPatchMutationOutcome,
  RequestPatchPayload,
  RequestPatchRule,
  RequestPatchUpdatePayload,
} from "@/services/types";

export type RequestPatchOwnerKind = "provider" | "model";
export type RequestPatchBadgeVariant =
  | "default"
  | "secondary"
  | "destructive"
  | "outline";

export interface RequestPatchRuleState {
  label: string;
  variant: RequestPatchBadgeVariant;
}

export interface RequestPatchRuleActions {
  createRule: (
    payload: RequestPatchPayload,
  ) => Promise<RequestPatchMutationOutcome>;
  updateRule: (
    ruleId: number,
    payload: RequestPatchUpdatePayload,
  ) => Promise<RequestPatchMutationOutcome>;
  deleteRule: (ruleId: number) => Promise<void>;
}

export type RequestPatchRuleStateResolver = (
  rule: RequestPatchRule,
) => RequestPatchRuleState;

export type RequestPatchRuleTraceResolver = (
  rule: RequestPatchRule,
) => string | null;
