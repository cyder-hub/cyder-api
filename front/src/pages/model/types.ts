import type { ModelSummaryItem } from "@/services/types";

export type ModelCapabilityKey =
  | "supports_streaming"
  | "supports_tools"
  | "supports_reasoning"
  | "supports_image_input"
  | "supports_embeddings"
  | "supports_rerank";

export interface ModelCapabilityItem {
  key: ModelCapabilityKey;
  labelKey: string;
}

export interface ModelSummaryCard {
  key: string;
  label: string;
  value: number;
}

export interface ModelPageState {
  filteredItems: ModelSummaryItem[];
  isPageEmpty: boolean;
  isSearchEmpty: boolean;
}
