import type { ModelSummaryItem } from "@/services/types";

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
