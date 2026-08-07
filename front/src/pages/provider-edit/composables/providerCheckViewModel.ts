import type { ProviderCheckResponse } from "@/services/types";

export interface CheckOption {
  value: number;
  label: string;
}

export interface CheckOptionsResult {
  options: CheckOption[];
  defaultSelectedValue: null;
}

export function buildCheckOptions<T>(
  items: T[],
  getLabel: (item: T, index: number) => string,
): CheckOptionsResult {
  return {
    options: items.map((item, index) => ({
      value: index,
      label: `#${index + 1} ${getLabel(item, index)}`,
    })),
    defaultSelectedValue: null,
  };
}

export function formatCheckSourceEvidence(
  result: ProviderCheckResponse,
): string {
  return `${result.profile_type} · source #${result.source_id}`;
}
