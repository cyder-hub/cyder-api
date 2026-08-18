import type {
  RequestPatchExplainStatus,
  RequestPatchVariantOrigin,
} from "@/services/types";

export type RequestPatchBadgeVariant =
  | "default"
  | "secondary"
  | "destructive"
  | "outline";

export interface RequestPatchStatusView {
  label: string;
  variant: RequestPatchBadgeVariant;
}

export interface RequestPatchLayerView {
  origin: RequestPatchVariantOrigin;
  status: RequestPatchExplainStatus;
  reason: string | null;
  variant_id: number | null;
  enabled: boolean;
  rule_count: number;
  expose_in_models: boolean;
}
