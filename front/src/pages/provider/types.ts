import type { ProviderRuntimeLevel, ProviderSummaryItem } from "@/services/types";

export type ProviderRuntimeLevelMap = Record<number, ProviderRuntimeLevel>;

export interface ProviderSummaryCard {
  key: string;
  label: string;
  value: number;
}

export type ProviderBadgeClassResolver = (provider: ProviderSummaryItem) => string;
export type ProviderLabelResolver = (provider: ProviderSummaryItem) => string;
export type RuntimeBadgeClassResolver = (level: ProviderRuntimeLevel) => string;
export type RuntimeLabelResolver = (level: ProviderRuntimeLevel) => string;
