<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { formatPriceFromNanos, nanosToMajorUnit } from "@/lib/utils";
import { Api } from "@/services/request";
import type { UsageStatItem, UsageStatsPeriod } from "@/store/types";
import ECharts from "./ECharts.vue";
import { Button } from "./ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "./ui/table";
import type { EChartsOption } from "echarts";
import type {
  DefaultLabelFormatterCallbackParams as CallbackDataParams,
  TooltipComponentFormatterCallbackParams as TopLevelFormatterParams,
} from "echarts/types/dist/option";

type TimeRange =
  | "last_1_hour"
  | "last_3_hours"
  | "last_6_hours"
  | "last_24_hours"
  | "today"
  | "yesterday"
  | "this_week"
  | "last_7_days"
  | "previous_week"
  | "this_month"
  | "last_30_days"
  | "previous_month"
  | "last_6_months"
  | "this_year"
  | "last_1_year";

type UsageMetric =
  | "total_input_tokens"
  | "total_output_tokens"
  | "total_reasoning_tokens"
  | "total_tokens"
  | "request_count"
  | "total_cost"
  | "success_rate"
  | "avg_latency"
  | "error_count";

type UsageGroupBy = "provider" | "model" | "api_key";

interface UsageStatsResponse {
  stats: UsageStatsPeriod[];
  interval: "month" | "day" | "hour" | "minute";
  startTime: Date;
  endTime: Date;
}

interface GroupSummaryRow {
  key: string;
  label: string;
  detail: string | null;
  valueText: string;
  sortValue: number;
}

type TooltipAxisParam = CallbackDataParams & {
  axisValue: string | number;
  marker: string;
  seriesName: string;
  value: [number, number];
};

const { t } = useI18n();
const timeRange = ref<TimeRange>("last_24_hours");
const selectedMetric = ref<UsageMetric>("total_tokens");
const selectedGroupBy = ref<UsageGroupBy>("model");
const selectedTopN = ref("6");
const chartType = ref<"line" | "bar">("line");
const usageData = ref<UsageStatsResponse | null>(null);
const isLoading = ref(false);
const error = ref<string | null>(null);
const chartHeight = ref(320);

const updateChartHeight = () => {
  if (typeof window === "undefined") return;
  chartHeight.value = window.innerWidth < 640 ? 320 : 400;
};

onMounted(() => {
  updateChartHeight();
  window.addEventListener("resize", updateChartHeight, { passive: true });
  window.addEventListener("orientationchange", updateChartHeight, {
    passive: true,
  });
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", updateChartHeight);
  window.removeEventListener("orientationchange", updateChartHeight);
});

const getTimeRangeDetails = (value: TimeRange) => {
  const now = new Date();
  let startTime: Date;
  let endTime = new Date(now);
  let interval: "month" | "day" | "hour" | "minute";

  switch (value) {
    case "last_1_hour":
      startTime = new Date(now.getTime() - 60 * 60 * 1000);
      interval = "minute";
      break;
    case "last_3_hours":
      startTime = new Date(now.getTime() - 3 * 60 * 60 * 1000);
      interval = "minute";
      break;
    case "last_6_hours":
      startTime = new Date(now.getTime() - 6 * 60 * 60 * 1000);
      interval = "hour";
      break;
    case "last_24_hours":
      startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
      interval = "hour";
      break;
    case "today":
      startTime = new Date(now);
      startTime.setHours(0, 0, 0, 0);
      interval = "hour";
      break;
    case "yesterday":
      startTime = new Date(now);
      startTime.setDate(now.getDate() - 1);
      startTime.setHours(0, 0, 0, 0);
      endTime = new Date(startTime);
      endTime.setHours(23, 59, 59, 999);
      interval = "hour";
      break;
    case "this_week": {
      startTime = new Date(now);
      const dayOfWeek = now.getDay();
      const diff = now.getDate() - dayOfWeek + (dayOfWeek === 0 ? -6 : 1);
      startTime.setDate(diff);
      startTime.setHours(0, 0, 0, 0);
      interval = "day";
      break;
    }
    case "last_7_days":
      startTime = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
      interval = "day";
      break;
    case "previous_week": {
      startTime = new Date(now);
      const dayOfWeek = now.getDay();
      const diff = now.getDate() - dayOfWeek + (dayOfWeek === 0 ? -6 : 1) - 7;
      startTime.setDate(diff);
      startTime.setHours(0, 0, 0, 0);
      endTime = new Date(startTime);
      endTime.setDate(startTime.getDate() + 6);
      endTime.setHours(23, 59, 59, 999);
      interval = "day";
      break;
    }
    case "this_month":
      startTime = new Date(now.getFullYear(), now.getMonth(), 1);
      interval = "day";
      break;
    case "last_30_days":
      startTime = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);
      interval = "day";
      break;
    case "previous_month":
      startTime = new Date(now.getFullYear(), now.getMonth() - 1, 1);
      endTime = new Date(now.getFullYear(), now.getMonth(), 0);
      endTime.setHours(23, 59, 59, 999);
      interval = "day";
      break;
    case "last_6_months":
      startTime = new Date(now);
      startTime.setMonth(now.getMonth() - 6);
      interval = "month";
      break;
    case "this_year":
      startTime = new Date(now.getFullYear(), 0, 1);
      interval = "month";
      break;
    case "last_1_year":
      startTime = new Date(now);
      startTime.setFullYear(now.getFullYear() - 1);
      interval = "month";
      break;
    default:
      startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
      interval = "hour";
  }

  return { startTime, endTime, interval };
};

const getApiMetric = (metric: UsageMetric) =>
  metric === "avg_latency" ? "avg_latency" : metric;

const fetchUsageStats = async () => {
  isLoading.value = true;
  error.value = null;
  try {
    const { startTime, endTime, interval } = getTimeRangeDetails(timeRange.value);
    const params = new URLSearchParams({
      interval,
      start_time: startTime.getTime().toString(),
      end_time: endTime.getTime().toString(),
      group_by: selectedGroupBy.value,
      metric: getApiMetric(selectedMetric.value),
      top_n: selectedTopN.value,
      include_others: "true",
    });
    const data = await Api.getUsageStats(params);
    usageData.value = { stats: data || [], interval, startTime, endTime };
  } catch (e: any) {
    error.value = e?.message || t("common.unknownError");
    usageData.value = null;
  } finally {
    isLoading.value = false;
  }
};

watch(
  [timeRange, selectedMetric, selectedGroupBy, selectedTopN],
  () => {
    fetchUsageStats();
  },
  { immediate: true },
);

const chartTypeOptions = computed(() => [
  { value: "line", label: t("dashboard.usageStats.chartTypes.line") },
  { value: "bar", label: t("dashboard.usageStats.chartTypes.bar") },
]);

const metricOptions = computed(() => [
  { value: "total_tokens", label: t("dashboard.usageStats.metrics.total_tokens") },
  { value: "request_count", label: t("dashboard.usageStats.metrics.request_count") },
  { value: "total_cost", label: t("dashboard.usageStats.metrics.total_cost") },
  { value: "success_rate", label: t("dashboard.usageStats.metrics.success_rate") },
  { value: "error_count", label: t("dashboard.usageStats.metrics.error_count") },
  { value: "avg_latency", label: t("dashboard.usageStats.metrics.avg_latency") },
  {
    value: "total_input_tokens",
    label: t("dashboard.usageStats.metrics.total_input_tokens"),
  },
  {
    value: "total_output_tokens",
    label: t("dashboard.usageStats.metrics.total_output_tokens"),
  },
  {
    value: "total_reasoning_tokens",
    label: t("dashboard.usageStats.metrics.total_reasoning_tokens"),
  },
]);

const groupByOptions = computed(() => [
  { value: "model", label: t("dashboard.usageStats.groupBy.model") },
  { value: "provider", label: t("dashboard.usageStats.groupBy.provider") },
  {
    value: "api_key",
    label: t("dashboard.usageStats.groupBy.api_key"),
  },
]);

const topNOptions = computed(() => [
  { value: "5", label: "Top 5" },
  { value: "6", label: "Top 6" },
  { value: "8", label: "Top 8" },
  { value: "10", label: "Top 10" },
]);

const timeRangeOptions = computed(() => [
  { value: "last_1_hour", label: t("dashboard.usageStats.timeRanges.last_1_hour") },
  { value: "last_3_hours", label: t("dashboard.usageStats.timeRanges.last_3_hours") },
  { value: "last_6_hours", label: t("dashboard.usageStats.timeRanges.last_6_hours") },
  { value: "last_24_hours", label: t("dashboard.usageStats.timeRanges.last_24_hours") },
  { value: "today", label: t("dashboard.usageStats.timeRanges.today") },
  { value: "yesterday", label: t("dashboard.usageStats.timeRanges.yesterday") },
  { value: "this_week", label: t("dashboard.usageStats.timeRanges.this_week") },
  { value: "last_7_days", label: t("dashboard.usageStats.timeRanges.last_7_days") },
  { value: "previous_week", label: t("dashboard.usageStats.timeRanges.previous_week") },
  { value: "this_month", label: t("dashboard.usageStats.timeRanges.this_month") },
  { value: "last_30_days", label: t("dashboard.usageStats.timeRanges.last_30_days") },
  { value: "previous_month", label: t("dashboard.usageStats.timeRanges.previous_month") },
  { value: "last_6_months", label: t("dashboard.usageStats.timeRanges.last_6_months") },
  { value: "this_year", label: t("dashboard.usageStats.timeRanges.this_year") },
  { value: "last_1_year", label: t("dashboard.usageStats.timeRanges.last_1_year") },
]);

const formatMetric = (value: number, metric: UsageMetric, currency?: string) => {
  switch (metric) {
    case "total_cost":
      return formatPriceFromNanos(value, currency, "-");
    case "success_rate":
      return `${(value * 100).toFixed(1)}%`;
    case "avg_latency":
      return `${Math.round(value).toLocaleString()} ms`;
    default:
      return value.toLocaleString();
  }
};

const formatCostAxisLabel = (value: number) =>
  new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 6,
    notation:
      Math.abs(nanosToMajorUnit(value)) >= 100000 ? "compact" : "standard",
  }).format(nanosToMajorUnit(value));

const formatAxisLabel = (value: number, metric: UsageMetric) => {
  if (metric === "total_cost") return formatCostAxisLabel(value);
  if (metric === "success_rate") return `${(value * 100).toFixed(0)}%`;
  if (metric === "avg_latency") return `${Math.round(value)}ms`;
  return value.toLocaleString();
};

const displayGroupLabel = (item: UsageStatItem) =>
  item.is_other ? t("dashboard.usageStats.others") : item.group_label || t("common.notAvailable");

const displayGroupDetail = (item: UsageStatItem) =>
  item.is_other ? null : item.group_detail;

const metricValueFromItem = (item: UsageStatItem, metric: UsageMetric) => {
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
      return item.avg_total_latency_ms ?? 0;
    case "error_count":
      return item.error_count;
  }
};

const isTooltipAxisParam = (
  param: CallbackDataParams,
): param is TooltipAxisParam =>
  "axisValue" in param &&
  Array.isArray(param.value) &&
  typeof param.marker === "string" &&
  typeof param.seriesName === "string";

const getTooltipRows = (params: TooltipAxisParam[]) =>
  params
    .filter((param) => Number(param.value?.[1] ?? 0) !== 0)
    .sort((a, b) => Number(b.value?.[1] ?? 0) - Number(a.value?.[1] ?? 0))
    .slice(0, 10);

const totalMetricSumText = computed(() => {
  if (!usageData.value?.stats.length) return "";
  const items = usageData.value.stats.flatMap((period) => period.data);
  if (!items.length) return "";

  if (selectedMetric.value === "total_cost") {
    const costSums: Record<string, number> = {};
    items.forEach((item) => {
      Object.entries(item.total_cost).forEach(([currency, amount]) => {
        costSums[currency] = (costSums[currency] || 0) + amount;
      });
    });
    return Object.entries(costSums)
      .map(([currency, amount]) => formatMetric(amount, "total_cost", currency))
      .join(" / ");
  }

  if (selectedMetric.value === "success_rate") {
    const requestCount = items.reduce((sum, item) => sum + item.request_count, 0);
    const successCount = items.reduce((sum, item) => sum + item.success_count, 0);
    return requestCount > 0
      ? formatMetric(successCount / requestCount, "success_rate")
      : "";
  }

  if (selectedMetric.value === "avg_latency") {
    const latencySampleCount = items.reduce(
      (sum, item) => sum + item.latency_sample_count,
      0,
    );
    const weightedLatency = items.reduce(
      (sum, item) =>
        sum + (item.avg_total_latency_ms ?? 0) * item.latency_sample_count,
      0,
    );
    return latencySampleCount > 0
      ? formatMetric(weightedLatency / latencySampleCount, "avg_latency")
      : "";
  }

  const total = items.reduce(
    (sum, item) => sum + metricValueFromItem(item, selectedMetric.value),
    0,
  );
  return total > 0 ? formatMetric(total, selectedMetric.value) : "";
});

const groupSummaryRows = computed<GroupSummaryRow[]>(() => {
  if (!usageData.value?.stats.length) return [];

  const groups = new Map<
    string,
    {
      label: string;
      detail: string | null;
      requestCount: number;
      successCount: number;
      errorCount: number;
      latencySampleCount: number;
      latencyWeighted: number;
      metrics: Record<string, number>;
      costMap: Record<string, number>;
    }
  >();

  usageData.value.stats.forEach((period) => {
    period.data.forEach((item) => {
      const key = item.group_key;
      const current = groups.get(key) ?? {
        label: displayGroupLabel(item),
        detail: displayGroupDetail(item),
        requestCount: 0,
        successCount: 0,
        errorCount: 0,
        latencySampleCount: 0,
        latencyWeighted: 0,
        metrics: {},
        costMap: {},
      };

      current.requestCount += item.request_count;
      current.successCount += item.success_count;
      current.errorCount += item.error_count;
      current.latencySampleCount += item.latency_sample_count;
      current.latencyWeighted +=
        (item.avg_total_latency_ms ?? 0) * item.latency_sample_count;
      current.metrics.total_input_tokens =
        (current.metrics.total_input_tokens || 0) + item.total_input_tokens;
      current.metrics.total_output_tokens =
        (current.metrics.total_output_tokens || 0) + item.total_output_tokens;
      current.metrics.total_reasoning_tokens =
        (current.metrics.total_reasoning_tokens || 0) + item.total_reasoning_tokens;
      current.metrics.total_tokens =
        (current.metrics.total_tokens || 0) + item.total_tokens;
      current.metrics.request_count =
        (current.metrics.request_count || 0) + item.request_count;
      current.metrics.error_count =
        (current.metrics.error_count || 0) + item.error_count;

      Object.entries(item.total_cost).forEach(([currency, amount]) => {
        current.costMap[currency] = (current.costMap[currency] || 0) + amount;
      });

      groups.set(key, current);
    });
  });

  return Array.from(groups.entries())
    .map(([key, group]) => {
      if (selectedMetric.value === "total_cost") {
        const valueText = Object.entries(group.costMap)
          .map(([currency, amount]) => formatMetric(amount, "total_cost", currency))
          .join(" / ");
        return {
          key,
          label: group.label,
          detail: group.detail,
          valueText,
          sortValue: Object.values(group.costMap).reduce((sum, value) => sum + value, 0),
        };
      }

      if (selectedMetric.value === "success_rate") {
        const ratio =
          group.requestCount > 0 ? group.successCount / group.requestCount : 0;
        return {
          key,
          label: group.label,
          detail: group.detail,
          valueText: formatMetric(ratio, "success_rate"),
          sortValue: ratio,
        };
      }

      if (selectedMetric.value === "avg_latency") {
        const value =
          group.latencySampleCount > 0
            ? group.latencyWeighted / group.latencySampleCount
            : 0;
        return {
          key,
          label: group.label,
          detail: group.detail,
          valueText: formatMetric(value, "avg_latency"),
          sortValue: value,
        };
      }

      const value = group.metrics[selectedMetric.value] || 0;
      return {
        key,
        label: group.label,
        detail: group.detail,
        valueText: formatMetric(value, selectedMetric.value),
        sortValue: value,
      };
    })
    .sort((a, b) => b.sortValue - a.sortValue)
    .slice(0, 10);
});

const chartOptions = computed<EChartsOption>(() => {
  if (!usageData.value) {
    return { title: { text: t("common.loading"), left: "center", top: "center" } };
  }

  const { stats, interval } = usageData.value;
  if (!stats.length) {
    return {
      title: {
        text: t("dashboard.usageStats.noData"),
        left: "center",
        top: "center",
      },
    };
  }

  const timeBuckets = Array.from(new Set(stats.map((period) => period.time))).sort(
    (a, b) => a - b,
  );

  const statsByTime = new Map(
    stats.map((period) => [
      period.time,
      new Map(period.data.map((item) => [item.group_key, item])),
    ]),
  );

  const seriesMap = new Map<
    string,
    {
      name: string;
      type: "line" | "bar";
      data: [number, number][];
      stack?: string;
      groupKey: string;
      currency?: string;
    }
  >();

  stats.forEach((period) => {
    period.data.forEach((item) => {
      const label = displayGroupLabel(item);
      if (selectedMetric.value === "total_cost") {
        Object.keys(item.total_cost).forEach((currency) => {
          const key = `${item.group_key}:${currency}`;
          if (!seriesMap.has(key)) {
            seriesMap.set(key, {
              name: `${label} (${currency})`,
              type: chartType.value,
              data: [],
              stack: chartType.value === "bar" ? currency : undefined,
              groupKey: item.group_key,
              currency,
            });
          }
        });
        return;
      }

      if (!seriesMap.has(item.group_key)) {
        seriesMap.set(item.group_key, {
          name: label,
          type: chartType.value,
          data: [],
          stack: chartType.value === "bar" ? "total" : undefined,
          groupKey: item.group_key,
        });
      }
    });
  });

  seriesMap.forEach((series) => {
    series.data = timeBuckets.map((bucketTime) => {
      const item = statsByTime.get(bucketTime)?.get(series.groupKey);
      if (!item) return [bucketTime, 0];
      if (selectedMetric.value === "total_cost") {
        return [bucketTime, item.total_cost[series.currency || ""] || 0];
      }
      return [bucketTime, metricValueFromItem(item, selectedMetric.value)];
    });
  });

  const finalSeries = Array.from(seriesMap.values()).filter((series) =>
    series.data.some((point) => point[1] !== 0),
  );

  if (!finalSeries.length) {
    return {
      title: {
        text: t("dashboard.usageStats.noData"),
        left: "center",
        top: "center",
      },
    };
  }

  return {
    tooltip: {
      trigger: "axis",
      axisPointer: {
        type: "cross",
        ...(selectedMetric.value === "total_cost"
          ? {
              label: {
                formatter: (params: any) =>
                  params?.axisDimension === "y" && typeof params?.value === "number"
                    ? formatCostAxisLabel(params.value)
                    : String(params?.value ?? ""),
              },
            }
          : {}),
      },
      formatter: (rawParams: TopLevelFormatterParams) => {
        const params = Array.isArray(rawParams) ? rawParams : [rawParams];
        const rows = getTooltipRows(params.filter(isTooltipAxisParam));
        if (!rows.length) return "";
        const date = new Date(rows[0].axisValue);

        return `${date.toLocaleString()}<br/>${rows
          .map((row) => {
            const currency =
              selectedMetric.value === "total_cost"
                ? row.seriesName.match(/ \((.*)\)$/)?.[1]
                : undefined;
            return `${row.marker} ${row.seriesName}: ${formatMetric(
              row.value[1],
              selectedMetric.value,
              currency,
            )}`;
          })
          .join("<br/>")}`;
      },
    },
    legend: {
      orient: "horizontal",
      left: 0,
      right: 0,
      top: 0,
      type: "scroll",
      data: finalSeries.map((series) => series.name),
      textStyle: { width: 120, overflow: "truncate" },
      tooltip: { show: true },
    },
    grid: { left: "4%", right: "4%", top: 72, bottom: 72, containLabel: true },
    xAxis: {
      type: "time",
      axisLabel: {
        hideOverlap: true,
        rotate: interval === "hour" || interval === "minute" ? 30 : 0,
        formatter:
          interval === "month"
            ? "{yyyy}-{MM}"
            : interval === "day"
              ? "{MM}-{dd}"
              : "{HH}:{mm}",
      },
    },
    yAxis: {
      type: "value",
      name: t(`dashboard.usageStats.metrics.${selectedMetric.value}`),
      axisLabel: {
        formatter: (value: number) => formatAxisLabel(value, selectedMetric.value),
      },
    },
    series: finalSeries.map((series) => ({
      ...series,
      smooth: chartType.value === "line",
    })),
    dataZoom: [
      { type: "slider", start: 0, end: 100, height: 20, bottom: 16 },
      { type: "inside", start: 0, end: 100 },
    ],
    toolbox: { feature: { saveAsImage: {} } },
  };
});
</script>

<template>
  <div class="app-stack-md rounded-xl bg-white p-4 sm:p-6">
    <div class="flex flex-col gap-3 sm:gap-4">
      <div class="flex flex-col gap-2 lg:flex-row lg:items-end lg:justify-between">
        <div class="min-w-0">
          <h2 class="text-lg font-semibold text-gray-900">
            {{ t("dashboard.usageStats.title") }}
          </h2>
          <p v-if="totalMetricSumText" class="mt-1 text-sm leading-6 text-gray-500">
            {{ t("dashboard.usageStats.total") }}: {{ totalMetricSumText }}
          </p>
          <p class="mt-1 text-xs text-gray-400">
            {{ t("dashboard.usageStats.topNHint", { topN: selectedTopN }) }}
          </p>
        </div>
      </div>

      <div
        class="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-[minmax(0,9rem)_minmax(0,13rem)_minmax(0,11rem)_minmax(0,10rem)_minmax(0,10rem)_auto]"
      >
        <Select v-model="chartType">
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="t('dashboard.usageStats.chartTypeLabel')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="opt in chartTypeOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>

        <Select v-model="selectedMetric">
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="t('dashboard.usageStats.metricLabel')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="opt in metricOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>

        <Select v-model="selectedGroupBy">
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="t('dashboard.usageStats.groupByLabel')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="opt in groupByOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>

        <Select v-model="selectedTopN">
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="t('dashboard.usageStats.topNLabel')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="opt in topNOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>

        <Select v-model="timeRange">
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="t('dashboard.usageStats.timeRangeLabel')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="opt in timeRangeOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>

        <Button
          @click="fetchUsageStats"
          variant="outline"
          size="sm"
          class="w-full sm:w-auto xl:min-w-28"
          :disabled="isLoading"
        >
          {{ t("common.refresh") }}
        </Button>
      </div>
    </div>

    <div
      v-if="isLoading"
      class="flex items-center justify-center rounded-lg border border-dashed border-gray-200 bg-gray-50/70"
      :style="{ height: `${chartHeight}px` }"
    >
      <p class="text-sm text-gray-500">{{ t("common.loading") }}</p>
    </div>
    <div
      v-else-if="error"
      class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-500"
    >
      <p>{{ t("dashboard.errorLoading", { error }) }}</p>
    </div>
    <div v-else-if="usageData" class="app-stack-md">
      <div class="rounded-lg border border-gray-200 bg-gray-50/30 p-2 sm:p-3">
        <ECharts :option="chartOptions" :style="{ height: `${chartHeight}px` }" />
      </div>

      <div v-if="groupSummaryRows.length > 0" class="app-stack-sm">
        <h3 class="text-base font-semibold text-gray-900 sm:text-lg">
          {{ t("dashboard.usageStats.summary.title") }}
        </h3>
        <div class="max-h-72 overflow-y-auto rounded-lg border border-gray-200">
          <Table>
            <TableHeader class="sticky top-0 z-10 bg-gray-50">
              <TableRow>
                <TableHead>{{ t("dashboard.usageStats.summary.entity") }}</TableHead>
                <TableHead>{{ t("dashboard.usageStats.summary.detail") }}</TableHead>
                <TableHead class="text-right">
                  {{ t(`dashboard.usageStats.metrics.${selectedMetric}`) }}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow v-for="row in groupSummaryRows" :key="row.key">
                <TableCell class="align-top font-medium text-gray-900">
                  {{ row.label }}
                </TableCell>
                <TableCell class="align-top text-xs text-gray-500">
                  {{ row.detail || "-" }}
                </TableCell>
                <TableCell class="text-right font-mono text-sm text-gray-900">
                  {{ row.valueText }}
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
      </div>
    </div>
  </div>
</template>
