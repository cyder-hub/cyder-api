<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watchEffect } from "vue";
import { useI18n } from "vue-i18n";
import { formatPriceFromNanos, nanosToMajorUnit } from "@/lib/utils";
import { Api } from "@/services/request";
import type { UsageStatsPeriod } from "@/store/types";
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

// Type definitions from the original component
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
  | "total_cost";
// Types imported from @/services/request
interface UsageStatsResponse {
  stats: UsageStatsPeriod[];
  interval: "month" | "day" | "hour" | "minute";
  startTime: Date;
  endTime: Date;
}

// State
const { t } = useI18n();
const timeRange = ref<TimeRange>("last_24_hours");
const selectedMetric = ref<UsageMetric>("total_tokens");
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

// Helper function (mostly unchanged)
const getTimeRangeDetails = (timeRange: TimeRange) => {
  const now = new Date();
  let startTime: Date;
  let endTime = new Date(now);
  let interval: "month" | "day" | "hour" | "minute";

  switch (timeRange) {
    case "last_1_hour":
      startTime = new Date(now.getTime() - 1 * 60 * 60 * 1000);
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
    case "this_week":
      startTime = new Date(now);
      const dayOfWeek = now.getDay(); // Sunday - 0, Monday - 1, ...
      const diff = now.getDate() - dayOfWeek + (dayOfWeek === 0 ? -6 : 1); // adjust when day is sunday
      startTime.setDate(diff);
      startTime.setHours(0, 0, 0, 0);
      interval = "day";
      break;
    case "last_7_days":
      startTime = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
      interval = "day";
      break;
    case "previous_week":
      startTime = new Date(now);
      const dayOfWeekForPrev = now.getDay();
      const diffForPrev =
        now.getDate() -
        dayOfWeekForPrev +
        (dayOfWeekForPrev === 0 ? -6 : 1) -
        7;
      startTime.setDate(diffForPrev);
      startTime.setHours(0, 0, 0, 0);
      endTime = new Date(startTime);
      endTime.setDate(startTime.getDate() + 6);
      endTime.setHours(23, 59, 59, 999);
      interval = "day";
      break;
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
      // fallback to last 24 hours
      startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
      interval = "hour";
  }
  return { startTime, endTime, interval };
};

// Data fetching
const fetchUsageStats = async () => {
  isLoading.value = true;
  error.value = null;
  try {
    const { startTime, endTime, interval } = getTimeRangeDetails(
      timeRange.value,
    );
    const params = new URLSearchParams({
      interval,
      start_time: startTime.getTime().toString(),
      end_time: endTime.getTime().toString(),
    });
    const data = await Api.getUsageStats(params);
    usageData.value = { stats: data || [], interval, startTime, endTime };
  } catch (e: any) {
    error.value = e.message || t("unknownError");
    usageData.value = null;
  } finally {
    isLoading.value = false;
  }
};

// Reactive watching
watchEffect(fetchUsageStats);

// Computed properties for UI options
const chartTypeOptions = computed(() => [
  { value: "line", label: t("dashboard.usageStats.chartTypes.line") },
  { value: "bar", label: t("dashboard.usageStats.chartTypes.bar") },
]);

const metricOptions = computed(() => [
  {
    value: "total_tokens",
    label: t("dashboard.usageStats.metrics.total_tokens"),
  },
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
  {
    value: "request_count",
    label: t("dashboard.usageStats.metrics.request_count"),
  },
  { value: "total_cost", label: t("dashboard.usageStats.metrics.total_cost") },
]);

const timeRangeOptions = computed(() => [
  {
    value: "last_1_hour",
    label: t("dashboard.usageStats.timeRanges.last_1_hour"),
  },
  {
    value: "last_3_hours",
    label: t("dashboard.usageStats.timeRanges.last_3_hours"),
  },
  {
    value: "last_6_hours",
    label: t("dashboard.usageStats.timeRanges.last_6_hours"),
  },
  {
    value: "last_24_hours",
    label: t("dashboard.usageStats.timeRanges.last_24_hours"),
  },
  { value: "today", label: t("dashboard.usageStats.timeRanges.today") },
  { value: "yesterday", label: t("dashboard.usageStats.timeRanges.yesterday") },
  { value: "this_week", label: t("dashboard.usageStats.timeRanges.this_week") },
  {
    value: "last_7_days",
    label: t("dashboard.usageStats.timeRanges.last_7_days"),
  },
  {
    value: "previous_week",
    label: t("dashboard.usageStats.timeRanges.previous_week"),
  },
  {
    value: "this_month",
    label: t("dashboard.usageStats.timeRanges.this_month"),
  },
  {
    value: "last_30_days",
    label: t("dashboard.usageStats.timeRanges.last_30_days"),
  },
  {
    value: "previous_month",
    label: t("dashboard.usageStats.timeRanges.previous_month"),
  },
  {
    value: "last_6_months",
    label: t("dashboard.usageStats.timeRanges.last_6_months"),
  },
  { value: "this_year", label: t("dashboard.usageStats.timeRanges.this_year") },
  {
    value: "last_1_year",
    label: t("dashboard.usageStats.timeRanges.last_1_year"),
  },
]);

// Formatting and data processing (computed properties)
const formatMetric = (
  value: number,
  metric: UsageMetric,
  currency?: string,
) => {
  if (metric === "total_cost") {
    return formatPriceFromNanos(value, currency, "-");
  }
  return value.toLocaleString();
};

const formatCostAxisLabel = (value: number) =>
  new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 6,
    notation: Math.abs(nanosToMajorUnit(value)) >= 100000 ? "compact" : "standard",
  }).format(nanosToMajorUnit(value));

type TooltipAxisParam = CallbackDataParams & {
  axisValue: string | number;
  marker: string;
  seriesName: string;
  value: [number, number];
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
  if (!usageData.value?.stats) return "";
  const metric = selectedMetric.value;

  if (metric !== "total_cost") {
    const sum = usageData.value.stats.reduce(
      (acc, period) =>
        acc +
        period.data.reduce((pAcc, item) => pAcc + (item as any)[metric], 0),
      0,
    );
    return sum > 0 ? formatMetric(sum, metric) : "";
  }

  const costSums: Record<string, number> = {};
  usageData.value.stats.forEach((p) =>
    p.data.forEach((i) => {
      for (const currency in i.total_cost) {
        costSums[currency] = (costSums[currency] || 0) + i.total_cost[currency];
      }
    }),
  );

  return Object.entries(costSums)
    .map(([currency, sum]) => formatMetric(sum, "total_cost", currency))
    .join(" / ");
});

const sortedModelSum = computed(() => {
  if (!usageData.value?.stats) return [];
  const metric = selectedMetric.value;

  if (metric !== "total_cost") {
    const sums = new Map<string, number>();
    usageData.value.stats.forEach((p) =>
      p.data.forEach((item) => {
        const name = `${item.provider_key || t("common.notAvailable")}/${item.model_name || t("common.notAvailable")}`;
        sums.set(name, (sums.get(name) || 0) + (item as any)[metric]);
      }),
    );
    return Array.from(sums.entries()).sort((a, b) => b[1] - a[1]);
  }

  const sums = new Map<string, Record<string, number>>();
  usageData.value.stats.forEach((p) =>
    p.data.forEach((item) => {
      const name = `${item.provider_key || t("common.notAvailable")}/${item.model_name || t("common.notAvailable")}`;
      const current = sums.get(name) || {};
      for (const currency in item.total_cost) {
        current[currency] =
          (current[currency] || 0) + item.total_cost[currency];
      }
      if (Object.keys(current).length > 0) sums.set(name, current);
    }),
  );

  const flatSums: { modelName: string; currency: string; sum: number }[] = [];
  sums.forEach((costMap, modelName) => {
    Object.entries(costMap).forEach(([currency, sum]) =>
      flatSums.push({ modelName, currency, sum }),
    );
  });
  return flatSums.sort((a, b) => b.sum - a.sum);
});

const chartOptions = computed<EChartsOption>(() => {
  if (!usageData.value) {
    return { title: { text: t("loading"), left: "center", top: "center" } };
  }
  const { stats, interval, startTime, endTime } = usageData.value;
  if (stats.length === 0) {
    return {
      title: {
        text: t("dashboard.usageStats.noData"),
        left: "center",
        top: "center",
      },
    };
  }

  const metric = selectedMetric.value;
  const type = chartType.value;

  const timeBuckets: number[] = [];
  let cursor = new Date(startTime);
  let step: "minute" | "month" | "day" | "hour" = interval;

  if (step === "minute") cursor.setSeconds(0, 0);
  else if (step === "hour") cursor.setMinutes(0, 0, 0);
  else if (step === "day") cursor.setHours(0, 0, 0, 0);
  else if (step === "month") {
    cursor.setDate(1);
    cursor.setHours(0, 0, 0, 0);
  }

  while (cursor <= endTime) {
    timeBuckets.push(cursor.getTime());
    if (step === "minute") cursor.setMinutes(cursor.getMinutes() + 1);
    else if (step === "hour") cursor.setHours(cursor.getHours() + 1);
    else if (step === "day") cursor.setDate(cursor.getDate() + 1);
    else if (step === "month") cursor.setMonth(cursor.getMonth() + 1);
  }

  const statsByTime = new Map(stats.map((p) => [p.time, p.data]));
  const seriesMap = new Map<
    string,
    {
      name: string;
      type: "line" | "bar";
      data: [number, number][];
      stack?: string;
    }
  >();

  // Prepare series map
  stats.forEach((p) =>
    p.data.forEach((item) => {
      const baseName = `${item.provider_key || t("common.notAvailable")}/${item.model_name || t("common.notAvailable")}`;
      if (metric === "total_cost") {
        Object.keys(item.total_cost).forEach((currency) => {
          const seriesName = `${baseName} (${currency})`;
          if (!seriesMap.has(seriesName)) {
            seriesMap.set(seriesName, {
              name: seriesName,
              type,
              data: [],
              stack: type === "bar" ? currency : undefined,
            });
          }
        });
      } else {
        if (!seriesMap.has(baseName)) {
          seriesMap.set(baseName, {
            name: baseName,
            type,
            data: [],
            stack: type === "bar" ? "total" : undefined,
          });
        }
      }
    }),
  );

  // Populate series data
  seriesMap.forEach((series, seriesName) => {
    series.data = timeBuckets.map((bucketTime) => {
      const periodData = statsByTime.get(bucketTime);
      let value = 0;
      if (periodData) {
        if (metric === "total_cost") {
          const match = seriesName.match(/(.*) \((.*)\)$/);
          const baseName = match?.[1];
          const currency = match?.[2];
          const item = periodData.find(
            (d) =>
              `${d.provider_key || t("common.notAvailable")}/${d.model_name || t("common.notAvailable")}` ===
              baseName,
          );
          if (item && currency && item.total_cost[currency]) {
            value = item.total_cost[currency];
          }
        } else {
          const item = periodData.find(
            (d) =>
              `${d.provider_key || t("common.notAvailable")}/${d.model_name || t("common.notAvailable")}` ===
              seriesName,
          );
          if (item) value = (item as any)[metric];
        }
      }
      return [bucketTime, value];
    });
  });

  const finalSeries = Array.from(seriesMap.values()).filter((s) =>
    s.data.some((d) => d[1] !== 0),
  );
  const legendData = finalSeries.map((s) => s.name);

  if (finalSeries.length === 0) {
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
        ...(metric === "total_cost"
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
        const rows = params.filter(isTooltipAxisParam);
        if (rows.length === 0) return "";

        const date = new Date(rows[0].axisValue);

        const filteredRows = getTooltipRows(rows);
        if (filteredRows.length === 0) return "";

        return (
          `${date.toLocaleString()}<br/>` +
          filteredRows
            .map((p) => {
              const currency =
                metric === "total_cost"
                  ? p.seriesName.match(/ \((.*)\)$/)?.[1]
                  : undefined;
              return `${p.marker} ${p.seriesName}: ${formatMetric(p.value[1], metric, currency)}`;
            })
            .join("<br/>")
        );
      },
    },
    legend: {
      orient: "horizontal",
      left: 0,
      right: 0,
      top: 0,
      data: legendData,
      type: "scroll",
      formatter: (name) =>
        metric === "total_cost" ? name.replace(/ \((.*)\)$/, "") : name,
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
      name: t(`dashboard.usageStats.metrics.${metric}`),
      ...(metric === "total_cost"
        ? {
            axisLabel: {
              formatter: (value: number) => formatCostAxisLabel(value),
            },
          }
        : {}),
    },
    series: finalSeries.map((s) => ({ ...s, smooth: type === "line" })),
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
          <p
            v-if="totalMetricSumText"
            class="mt-1 text-sm leading-6 text-gray-500"
          >
            {{ t("dashboard.usageStats.total") }}: {{ totalMetricSumText }}
          </p>
        </div>
      </div>

      <div class="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-[minmax(0,9rem)_minmax(0,13rem)_minmax(0,13rem)_auto] xl:items-end">
        <Select v-model="chartType">
          <SelectTrigger class="w-full">
            <SelectValue placeholder="Chart Type" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="opt in chartTypeOptions"
              :key="opt.value"
              :value="opt.value"
              >{{ opt.label }}</SelectItem
            >
          </SelectContent>
        </Select>
        <Select v-model="selectedMetric">
          <SelectTrigger class="w-full">
            <SelectValue placeholder="Metric" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="opt in metricOptions"
              :key="opt.value"
              :value="opt.value"
              >{{ opt.label }}</SelectItem
            >
          </SelectContent>
        </Select>
        <Select v-model="timeRange">
          <SelectTrigger class="w-full">
            <SelectValue placeholder="Time Range" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="opt in timeRangeOptions"
              :key="opt.value"
              :value="opt.value"
              >{{ opt.label }}</SelectItem
            >
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

    <div v-if="isLoading" class="flex items-center justify-center rounded-lg border border-dashed border-gray-200 bg-gray-50/70" :style="{ height: `${chartHeight}px` }">
      <p class="text-sm text-gray-500">{{ t("loading") }}</p>
    </div>
    <div v-else-if="error" class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-500">
      <p>{{ t("dashboard.errorLoading", { error: error }) }}</p>
    </div>
    <div v-else-if="usageData" class="app-stack-md">
      <div class="rounded-lg border border-gray-200 bg-gray-50/30 p-2 sm:p-3">
        <ECharts :option="chartOptions" :style="{ height: `${chartHeight}px` }" />
      </div>
      <div v-if="sortedModelSum.length > 0" class="app-stack-sm">
        <h3 class="text-base font-semibold text-gray-900 sm:text-lg">
          {{ t("dashboard.usageStats.summary.title") }}
        </h3>
        <div class="max-h-72 overflow-y-auto rounded-lg border border-gray-200">
          <Table>
            <TableHeader class="bg-gray-50 sticky top-0 z-10">
              <TableRow>
                <TableHead>
                  {{ t("dashboard.usageStats.summary.model") }}
                </TableHead>
                <TableHead>
                  {{ t(`dashboard.usageStats.metrics.${selectedMetric}`) }}
                  <span v-if="selectedMetric !== 'total_cost'">
                    ({{ t("dashboard.usageStats.total") }})</span
                  >
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow
                v-for="item in sortedModelSum"
                :key="
                  Array.isArray(item) ? item[0] : item.modelName + item.currency
                "
              >
                <TableCell class="font-medium text-gray-900">
                  {{ Array.isArray(item) ? item[0] : item.modelName }}
                </TableCell>
                <TableCell class="text-gray-500">
                  {{
                    Array.isArray(item)
                      ? formatMetric(item[1], selectedMetric)
                      : formatMetric(item.sum, "total_cost", item.currency)
                  }}
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
      </div>
    </div>
  </div>
</template>
