<script setup lang="ts">
import { ref, computed, watchEffect } from "vue";
import { useI18n } from "vue-i18n";
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
import type { EChartsOption } from "echarts";

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
  | "input_tokens"
  | "output_tokens"
  | "reasoning_tokens"
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
    value: "input_tokens",
    label: t("dashboard.usageStats.metrics.input_tokens"),
  },
  {
    value: "output_tokens",
    label: t("dashboard.usageStats.metrics.output_tokens"),
  },
  {
    value: "reasoning_tokens",
    label: t("dashboard.usageStats.metrics.reasoning_tokens"),
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
    const amount = value / 1_000_000_000;
    if (currency === "CNY") return `¥${amount.toFixed(6)}`;
    try {
      return new Intl.NumberFormat(undefined, {
        style: "currency",
        currency: currency || "USD",
        minimumFractionDigits: 2,
        maximumFractionDigits: 6,
      }).format(amount);
    } catch (e) {
      return `${currency || "$"} ${amount.toFixed(6)}`;
    }
  }
  return value.toLocaleString();
};

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
            value = item.total_cost[currency] / 1_000_000_000;
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
      axisPointer: { type: "cross" },
      formatter: (params: any) => {
        const date = new Date(params[0].axisValue);
        return (
          `${date.toLocaleString()}<br/>` +
          params
            .map((p: any) => {
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
      orient: "vertical",
      right: 10,
      top: "center",
      data: legendData,
      type: "scroll",
      formatter: (name) =>
        metric === "total_cost" ? name.replace(/ \((.*)\)$/, "") : name,
      textStyle: { width: 180, overflow: "truncate" },
      tooltip: { show: true },
    },
    grid: { left: "3%", right: 230, bottom: "10%", containLabel: true },
    xAxis: {
      type: "time",
      axisLabel: {
        formatter:
          interval === "month"
            ? "{yyyy}-{MM}"
            : interval === "day"
              ? "{MM}-{dd}"
              : "{HH}:{mm}",
      },
    },
    yAxis: { type: "value", name: t(`dashboard.usageStats.metrics.${metric}`) },
    series: finalSeries.map((s) => ({ ...s, smooth: type === "line" })),
    dataZoom: [
      { type: "slider", start: 0, end: 100 },
      { type: "inside", start: 0, end: 100 },
    ],
    toolbox: { feature: { saveAsImage: {} } },
  };
});
</script>

<template>
  <div class="mt-6 bg-white dark:bg-gray-800 p-6 rounded-lg shadow-md">
    <div class="flex flex-wrap justify-between items-center gap-4 mb-4">
      <div class="flex items-baseline space-x-4">
        <h2 class="text-xl font-semibold text-gray-700 dark:text-gray-200">
          {{ t("dashboard.usageStats.title") }}
        </h2>
        <span
          v-if="totalMetricSumText"
          class="text-lg font-medium text-gray-600 dark:text-gray-300"
        >
          {{ t("dashboard.usageStats.total") }}: {{ totalMetricSumText }}
        </span>
      </div>
      <div class="flex items-center space-x-2">
        <Select v-model="chartType">
          <SelectTrigger class="w-32">
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
          <SelectTrigger class="w-48">
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
          <SelectTrigger class="w-48">
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
          :disabled="isLoading"
        >
          {{ t("common.refresh") }}
        </Button>
      </div>
    </div>

    <div v-if="isLoading" class="flex justify-center items-center h-[400px]">
      <p>{{ t("loading") }}</p>
    </div>
    <div v-else-if="error" class="text-red-500">
      <p>{{ t("dashboard.errorLoading", { error: error }) }}</p>
    </div>
    <div v-else-if="usageData">
      <ECharts :option="chartOptions" style="height: 400px" />
      <div v-if="sortedModelSum.length > 0" class="mt-4">
        <h3 class="text-lg font-semibold text-gray-700 dark:text-gray-200 mb-2">
          {{ t("dashboard.usageStats.summary.title") }}
        </h3>
        <div
          class="max-h-60 overflow-y-auto border border-gray-200 dark:border-gray-700 rounded-md"
        >
          <table
            class="min-w-full divide-y divide-gray-200 dark:divide-gray-700"
          >
            <thead class="bg-gray-50 dark:bg-gray-700 sticky top-0 z-10">
              <tr>
                <th
                  scope="col"
                  class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-300 uppercase tracking-wider"
                >
                  {{ t("dashboard.usageStats.summary.model") }}
                </th>
                <th
                  scope="col"
                  class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-300 uppercase tracking-wider"
                >
                  {{ t(`dashboard.usageStats.metrics.${selectedMetric}`) }}
                  <span v-if="selectedMetric !== 'total_cost'">
                    ({{ t("dashboard.usageStats.total") }})</span
                  >
                </th>
              </tr>
            </thead>
            <tbody
              class="bg-white dark:bg-gray-800 divide-y divide-gray-200 dark:divide-gray-700"
            >
              <tr
                v-for="item in sortedModelSum"
                :key="
                  Array.isArray(item) ? item[0] : item.modelName + item.currency
                "
              >
                <td
                  class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900 dark:text-gray-100"
                >
                  {{ Array.isArray(item) ? item[0] : item.modelName }}
                </td>
                <td
                  class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400"
                >
                  {{
                    Array.isArray(item)
                      ? formatMetric(item[1], selectedMetric)
                      : formatMetric(item.sum, "total_cost", item.currency)
                  }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>
