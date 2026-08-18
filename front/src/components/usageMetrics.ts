import type { UsageStatItem } from "@/services/types";

export type UsageMetric =
  | "total_input_tokens"
  | "total_output_tokens"
  | "total_reasoning_tokens"
  | "total_tokens"
  | "request_count"
  | "total_cost"
  | "success_rate"
  | "avg_latency"
  | "avg_time_to_first_response_body"
  | "avg_ttft"
  | "error_count";

export type AverageUsageMetric =
  | "avg_latency"
  | "avg_time_to_first_response_body"
  | "avg_ttft";

export interface WeightedUsageAverage {
  average: number | null;
  sampleCount: number;
}

export function isAverageUsageMetric(metric: UsageMetric): metric is AverageUsageMetric {
  return (
    metric === "avg_latency" ||
    metric === "avg_time_to_first_response_body" ||
    metric === "avg_ttft"
  );
}

export function usageMetricSampleCount(
  item: UsageStatItem,
  metric: UsageMetric,
): number | null {
  switch (metric) {
    case "avg_latency":
      return item.total_latency_sample_count;
    case "avg_time_to_first_response_body":
      return item.time_to_first_response_body_sample_count;
    case "avg_ttft":
      return item.ttft_sample_count;
    default:
      return null;
  }
}

export function usageMetricValue(
  item: UsageStatItem,
  metric: UsageMetric,
): number | null {
  switch (metric) {
    case "total_input_tokens":
      return item.total_input_tokens;
    case "total_output_tokens":
      return item.total_output_tokens;
    case "total_reasoning_tokens":
      return item.total_reasoning_tokens;
    case "total_tokens":
      return item.total_tokens;
    case "request_count":
      return item.request_count;
    case "total_cost":
      return Object.values(item.total_cost).reduce((sum, value) => sum + value, 0);
    case "success_rate":
      return item.success_rate ?? 0;
    case "avg_latency":
      return item.avg_total_latency_ms;
    case "avg_time_to_first_response_body":
      return item.avg_time_to_first_response_body_ms;
    case "avg_ttft":
      return item.avg_ttft_ms;
    case "error_count":
      return item.error_count;
  }
}

export function weightedUsageAverage(
  items: ReadonlyArray<UsageStatItem>,
  metric: AverageUsageMetric,
): WeightedUsageAverage {
  let sampleCount = 0;
  let weightedSum = 0;

  for (const item of items) {
    const value = usageMetricValue(item, metric);
    const count = usageMetricSampleCount(item, metric) ?? 0;
    if (value == null || count <= 0) {
      continue;
    }
    sampleCount += count;
    weightedSum += value * count;
  }

  return {
    average: sampleCount > 0 ? weightedSum / sampleCount : null,
    sampleCount,
  };
}

