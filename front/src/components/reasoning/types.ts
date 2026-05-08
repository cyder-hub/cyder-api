import type {
  ModelReasoningConfigPayload,
  ProviderReasoningConfigPreviewPayload,
  ReasoningConfigCatalog,
  ReasoningConfigPreview,
  ReasoningConfigResponse,
} from "@/services/types";

export type ReasoningOwnerKind = "provider" | "model";

export type ReasoningDraftPreviewPayload =
  | ProviderReasoningConfigPreviewPayload
  | ModelReasoningConfigPayload;

export type ReasoningUpdatePayload =
  | ProviderReasoningConfigPreviewPayload
  | ModelReasoningConfigPayload;

export interface ReasoningConfigActions {
  getCatalog: () => Promise<ReasoningConfigCatalog>;
  getConfig: (ownerId: number | string) => Promise<ReasoningConfigResponse>;
  previewSaved: (ownerId: number | string) => Promise<ReasoningConfigPreview>;
  previewDraft: (
    ownerId: number | string,
    payload: ReasoningDraftPreviewPayload,
  ) => Promise<ReasoningConfigPreview>;
  updateConfig: (
    ownerId: number | string,
    payload: ReasoningUpdatePayload,
  ) => Promise<ReasoningConfigResponse | void>;
  deleteConfig: (ownerId: number | string) => Promise<void>;
}
