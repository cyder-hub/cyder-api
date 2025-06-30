import { createResource, Show, createSignal, createMemo, For } from 'solid-js';
import { useI18n } from '../i18n';
import { request } from '../services/api';
import * as echarts from 'echarts/core';
import type { EChartsOption } from 'echarts';
import { LineChart, BarChart } from 'echarts/charts';
import {
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent,
  DataZoomComponent,
  ToolboxComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import ECharts from './ECharts';
import { Button } from './ui/Button';
import { Select } from './ui/Select';

echarts.use([
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent,
  DataZoomComponent,
  ToolboxComponent,
  LineChart,
  BarChart,
  CanvasRenderer,
]);

type TimeRange = 'last_1_hour' | 'last_3_hours' | 'last_6_hours' | 'last_24_hours' | 'today' | 'yesterday' | 'this_week' | 'last_7_days' | 'previous_week' | 'this_month' | 'last_30_days' | 'previous_month' | 'last_6_months' | 'this_year' | 'last_1_year';

const getTimeRangeDetails = (timeRange: TimeRange): { startTime: Date, endTime: Date, interval: 'month' | 'day' | 'hour' | 'minute' } => {
    const now = new Date();
    let startTime: Date;
    let endTime = new Date(now);
    let interval: 'month' | 'day' | 'hour' | 'minute';

    switch (timeRange) {
        case 'last_1_hour':
            startTime = new Date(now.getTime() - 1 * 60 * 60 * 1000);
            interval = 'minute';
            break;
        case 'last_3_hours':
            startTime = new Date(now.getTime() - 3 * 60 * 60 * 1000);
            interval = 'minute';
            break;
        case 'last_6_hours':
            startTime = new Date(now.getTime() - 6 * 60 * 60 * 1000);
            interval = 'hour';
            break;
        case 'last_24_hours':
            startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
            interval = 'hour';
            break;
        case 'today':
            startTime = new Date(now);
            startTime.setHours(0, 0, 0, 0);
            interval = 'hour';
            break;
        case 'yesterday':
            startTime = new Date(now);
            startTime.setDate(now.getDate() - 1);
            startTime.setHours(0, 0, 0, 0);
            endTime = new Date(startTime);
            endTime.setHours(23, 59, 59, 999);
            interval = 'hour';
            break;
        case 'this_week':
            startTime = new Date(now);
            const dayOfWeek = now.getDay(); // Sunday - 0, Monday - 1, ...
            const diff = now.getDate() - dayOfWeek + (dayOfWeek === 0 ? -6 : 1); // adjust when day is sunday
            startTime.setDate(diff);
            startTime.setHours(0, 0, 0, 0);
            interval = 'day';
            break;
        case 'last_7_days':
            startTime = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
            interval = 'day';
            break;
        case 'previous_week':
            startTime = new Date(now);
            const dayOfWeekForPrev = now.getDay();
            const diffForPrev = now.getDate() - dayOfWeekForPrev + (dayOfWeekForPrev === 0 ? -6 : 1) - 7;
            startTime.setDate(diffForPrev);
            startTime.setHours(0, 0, 0, 0);
            endTime = new Date(startTime);
            endTime.setDate(startTime.getDate() + 6);
            endTime.setHours(23, 59, 59, 999);
            interval = 'day';
            break;
        case 'this_month':
            startTime = new Date(now.getFullYear(), now.getMonth(), 1);
            interval = 'day';
            break;
        case 'last_30_days':
            startTime = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);
            interval = 'day';
            break;
        case 'previous_month':
            startTime = new Date(now.getFullYear(), now.getMonth() - 1, 1);
            endTime = new Date(now.getFullYear(), now.getMonth(), 0);
            endTime.setHours(23, 59, 59, 999);
            interval = 'day';
            break;
        case 'last_6_months':
            startTime = new Date(now);
            startTime.setMonth(now.getMonth() - 6);
            interval = 'month';
            break;
        case 'this_year':
            startTime = new Date(now.getFullYear(), 0, 1);
            interval = 'month';
            break;
        case 'last_1_year':
            startTime = new Date(now);
            startTime.setFullYear(now.getFullYear() - 1);
            interval = 'month';
            break;
        default:
            // fallback to last 24 hours
            startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
            interval = 'hour';
    }
    return { startTime, endTime, interval };
};

interface UsageStatItem {
    provider_id: number | null;
    model_id: number | null;
    provider_key: string | null;
    model_name: string | null;
    real_model_name: string | null;
    prompt_tokens: number;
    completion_tokens: number;
    reasoning_tokens: number;
    total_tokens: number;
    request_count: number;
    total_cost: Record<string, number>;
}

interface UsageStatsPeriod {
    time: number; // Timestamp for the beginning of the period (milliseconds)
    data: UsageStatItem[];
}

type UsageMetric = 'prompt_tokens' | 'completion_tokens' | 'reasoning_tokens' | 'total_tokens' | 'request_count' | 'total_cost';

const fetchUsageStats = async (timeRange: TimeRange): Promise<{ stats: UsageStatsPeriod[], interval: 'month' | 'day' | 'hour' | 'minute', startTime: Date, endTime: Date }> => {
    try {
        const { startTime, endTime, interval } = getTimeRangeDetails(timeRange);

        const params = new URLSearchParams({
            interval,
            start_time: startTime.getTime().toString(),
            end_time: endTime.getTime().toString(),
        });
        const data = await request(`/ai/manager/api/system/usage_stats?${params.toString()}`);
        return { stats: data || [], interval, startTime, endTime };
    } catch (error) {
        console.error("Failed to fetch usage stats:", error);
        throw error;
    }
};

export default function UsageChart() {
    const [t] = useI18n();
    const [timeRange, setTimeRange] = createSignal<TimeRange>('last_24_hours');
    const [selectedMetric, setSelectedMetric] = createSignal<UsageMetric>('total_tokens');
    const [chartType, setChartType] = createSignal<'line' | 'bar'>('line');
    const [usageStats, { refetch: refetchUsageStats }] = createResource(timeRange, fetchUsageStats, { storage: createSignal });

    const chartTypeOptions = createMemo(() => [
        { value: 'line' as const, label: t('dashboard.usageStats.chartTypes.line') },
        { value: 'bar' as const, label: t('dashboard.usageStats.chartTypes.bar') }
    ]);

    const metricOptions = createMemo(() => ([
        { value: 'total_tokens' as const, label: t('dashboard.usageStats.metrics.total_tokens') },
        { value: 'prompt_tokens' as const, label: t('dashboard.usageStats.metrics.prompt_tokens') },
        { value: 'completion_tokens' as const, label: t('dashboard.usageStats.metrics.completion_tokens') },
        { value: 'reasoning_tokens' as const, label: t('dashboard.usageStats.metrics.reasoning_tokens') },
        { value: 'request_count' as const, label: t('dashboard.usageStats.metrics.request_count') },
        { value: 'total_cost' as const, label: t('dashboard.usageStats.metrics.total_cost') }
    ]));

    const timeRangeOptions = createMemo(() => ([
        { value: 'last_1_hour' as const, label: t('dashboard.usageStats.timeRanges.last_1_hour') },
        { value: 'last_3_hours' as const, label: t('dashboard.usageStats.timeRanges.last_3_hours') },
        { value: 'last_6_hours' as const, label: t('dashboard.usageStats.timeRanges.last_6_hours') },
        { value: 'last_24_hours' as const, label: t('dashboard.usageStats.timeRanges.last_24_hours') },
        { value: 'today' as const, label: t('dashboard.usageStats.timeRanges.today') },
        { value: 'yesterday' as const, label: t('dashboard.usageStats.timeRanges.yesterday') },
        { value: 'this_week' as const, label: t('dashboard.usageStats.timeRanges.this_week') },
        { value: 'last_7_days' as const, label: t('dashboard.usageStats.timeRanges.last_7_days') },
        { value: 'previous_week' as const, label: t('dashboard.usageStats.timeRanges.previous_week') },
        { value: 'this_month' as const, label: t('dashboard.usageStats.timeRanges.this_month') },
        { value: 'last_30_days' as const, label: t('dashboard.usageStats.timeRanges.last_30_days') },
        { value: 'previous_month' as const, label: t('dashboard.usageStats.timeRanges.previous_month') },
        { value: 'last_6_months' as const, label: t('dashboard.usageStats.timeRanges.last_6_months') },
        { value: 'this_year' as const, label: t('dashboard.usageStats.timeRanges.this_year') },
        { value: 'last_1_year' as const, label: t('dashboard.usageStats.timeRanges.last_1_year') }
    ]));

    const formatMetric = (value: number, metric: UsageMetric, currency?: string) => {
        if (metric === 'total_cost' && currency) {
            if (currency === 'CNY') {
                return `¥${value.toFixed(6)}`;
            }
            try {
                // Format as currency, e.g., $1,234.56
                return new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: 2, maximumFractionDigits: 6 }).format(value);
            } catch (e) {
                // Fallback for invalid currency code
                return `${currency} ${value.toFixed(6)}`;
            }
        }
        if (metric === 'total_cost') {
            return `$${value.toFixed(6)}`; // Fallback if no currency
        }
        return value.toLocaleString();
    };

    const totalMetricSumText = createMemo(() => {
        const data = usageStats();
        const metric = selectedMetric();
        if (!data || !data.stats) {
            return '';
        }

        if (metric !== 'total_cost') {
            const sum = data.stats.reduce((acc, period) => {
                return acc + period.data.reduce((periodAcc, item) => {
                    return periodAcc + (item as any)[metric];
                }, 0);
            }, 0);
            return sum > 0 ? formatMetric(sum, metric) : '';
        }

        // For total_cost
        const costSums: Record<string, number> = {};
        data.stats.forEach(period => {
            period.data.forEach(item => {
                for (const currency in item.total_cost) {
                    costSums[currency] = (costSums[currency] || 0) + item.total_cost[currency];
                }
            });
        });

        return Object.entries(costSums).map(([currency, sum]) => {
            return formatMetric(sum / 1_000_000_000, 'total_cost', currency);
        }).join(' / ');
    });

    const perModelMetricSum = createMemo(() => {
        const data = usageStats();
        const metric = selectedMetric();
        if (!data || !data.stats) {
            return new Map();
        }

        if (metric !== 'total_cost') {
            const sums = new Map<string, number>();
            data.stats.forEach(period => {
                period.data.forEach(item => {
                    const seriesName = `${item.provider_key || t('common.notAvailable')}/${item.model_name || t('common.notAvailable')}`;
                    const currentSum = sums.get(seriesName) || 0;
                    sums.set(seriesName, currentSum + (item as any)[metric]);
                });
            });
            return sums;
        }

        // For total_cost
        const sums = new Map<string, Record<string, number>>();
        data.stats.forEach(period => {
            period.data.forEach(item => {
                const seriesName = `${item.provider_key || t('common.notAvailable')}/${item.model_name || t('common.notAvailable')}`;
                const currentSums = sums.get(seriesName) || {};
                for (const currency in item.total_cost) {
                    currentSums[currency] = (currentSums[currency] || 0) + item.total_cost[currency];
                }
                if (Object.keys(currentSums).length > 0) {
                    sums.set(seriesName, currentSums);
                }
            });
        });

        sums.forEach((costMap) => {
            for (const currency in costMap) {
                costMap[currency] /= 1_000_000_000;
            }
        });

        return sums;
    });

    const sortedModelSum = createMemo(() => {
        const sums = perModelMetricSum();
        const metric = selectedMetric();

        if (metric !== 'total_cost') {
            return Array.from((sums as Map<string, number>).entries()).sort((a, b) => b[1] - a[1]);
        }

        // For total_cost
        const flatSums: { modelName: string, currency: string, sum: number }[] = [];
        (sums as Map<string, Record<string, number>>).forEach((costMap, modelName) => {
            Object.entries(costMap).forEach(([currency, sum]) => {
                flatSums.push({ modelName, currency, sum });
            });
        });

        // Sort by sum descending
        return flatSums.sort((a, b) => b.sum - a.sum);
    });

    const chartOptions = (): EChartsOption => {
        const data = usageStats();
        const metric = selectedMetric();
        const type = chartType();

        if (!data) {
            return {
                title: { text: t('loading'), left: 'center', top: 'center' }
            };
        }

        const { stats, interval: currentInterval, startTime, endTime } = data;

        const timeBuckets: number[] = [];
        let cursor = new Date(startTime);

        switch (currentInterval) {
            case 'minute':
                cursor = new Date(startTime.getFullYear(), startTime.getMonth(), startTime.getDate(), startTime.getHours(), startTime.getMinutes());
                while (cursor <= endTime) {
                    timeBuckets.push(cursor.getTime());
                    cursor.setMinutes(cursor.getMinutes() + 1);
                }
                break;
            case 'month':
                cursor = new Date(startTime.getFullYear(), startTime.getMonth(), 1);
                while (cursor <= endTime) {
                    timeBuckets.push(cursor.getTime());
                    cursor.setMonth(cursor.getMonth() + 1);
                }
                break;
            case 'day':
                cursor = new Date(startTime.getFullYear(), startTime.getMonth(), startTime.getDate());
                while (cursor <= endTime) {
                    timeBuckets.push(cursor.getTime());
                    cursor.setDate(cursor.getDate() + 1);
                }
                break;
            case 'hour':
                cursor = new Date(startTime.getFullYear(), startTime.getMonth(), startTime.getDate(), startTime.getHours());
                while (cursor <= endTime) {
                    timeBuckets.push(cursor.getTime());
                    cursor.setHours(cursor.getHours() + 1);
                }
                break;
        }

        if (stats.length === 0) {
            return {
                title: {
                    text: t('dashboard.usageStats.noData'),
                    left: 'center',
                    top: 'center'
                }
            };
        }

        const statsByTime = new Map<number, UsageStatItem[]>();
        stats.forEach(period => {
            statsByTime.set(period.time, period.data);
        });

        const seriesMap = new Map<string, { name: string; type: 'line' | 'bar'; data: [number, number][]; stack?: string }>();

        if (metric === 'total_cost') {
            for (const period of stats) {
                for (const item of period.data) {
                    const baseSeriesName = `${item.provider_key || t('common.notAvailable')}/${item.model_name || t('common.notAvailable')}`;
                    for (const currency in item.total_cost) {
                        const seriesName = `${baseSeriesName} (${currency})`;
                        if (!seriesMap.has(seriesName)) {
                            seriesMap.set(seriesName, { name: seriesName, type, data: [], stack: type === 'bar' ? currency : undefined });
                        }
                    }
                }
            }
        } else {
            for (const period of stats) {
                for (const item of period.data) {
                    const seriesName = `${item.provider_key || t('common.notAvailable')}/${item.model_name || t('common.notAvailable')}`;
                    if (!seriesMap.has(seriesName)) {
                        seriesMap.set(seriesName, { name: seriesName, type, data: [], stack: type === 'bar' ? 'total' : undefined });
                    }
                }
            }
        }

        const keysToDelete: string[] = [];
        seriesMap.forEach((series, seriesName) => {
            let allZero = true;
            series.data = timeBuckets.map(bucketTime => {
                const periodData = statsByTime.get(bucketTime);
                let value = 0;
                if (periodData) {
                    if (metric === 'total_cost') {
                        const match = seriesName.match(/(.*) \((.*)\)$/);
                        if (match) {
                            const [, baseName, currency] = match;
                            const item = periodData.find(d =>
                                `${d.provider_key || t('common.notAvailable')}/${d.model_name || t('common.notAvailable')}` === baseName
                            );
                            if (item && item.total_cost[currency]) {
                                value = item.total_cost[currency] / 1_000_000_000;
                            }
                        }
                    } else {
                        const item = periodData.find(d =>
                            `${d.provider_key || t('common.notAvailable')}/${d.model_name || t('common.notAvailable')}` === seriesName
                        );
                        if (item) {
                            value = (item as any)[metric];
                        }
                    }
                }
                if (value !== 0) {
                    allZero = false;
                }
                return [bucketTime, value];
            });

            if (allZero) {
                keysToDelete.push(seriesName);
            }
        });

        keysToDelete.forEach(key => seriesMap.delete(key));

        const series = Array.from(seriesMap.values());
        const legendData = Array.from(seriesMap.keys());

        if (series.length === 0) {
            return {
                title: {
                    text: t('dashboard.usageStats.noData'),
                    left: 'center',
                    top: 'center'
                }
            };
        }

        let xAxisFormatter: string;
        switch (currentInterval) {
            case 'month':
                xAxisFormatter = '{yyyy}-{MM}';
                break;
            case 'day':
                xAxisFormatter = '{MM}-{dd}';
                break;
            case 'minute':
            case 'hour':
                xAxisFormatter = '{HH}:{mm}';
                break;
        }

        return {
            tooltip: {
                trigger: 'axis',
                axisPointer: {
                    type: 'cross'
                },
                formatter: (params: any) => {
                    if (!Array.isArray(params) || params.length === 0) {
                        return '';
                    }
                    const date = new Date(params[0].axisValue as number);
                    let tooltipText = `${date.toLocaleString()}<br/>`;
                    params.forEach((param: any) => {
                        let displayName = param.seriesName as string;
                        const value = param.value as [number, number];
                        let currency: string | undefined;
                        if (metric === 'total_cost') {
                            const match = displayName.match(/ \((.*)\)$/);
                            if (match) {
                                currency = match[1];
                                displayName = displayName.replace(/ \((.*)\)$/, '');
                            }
                        }
                        tooltipText += `${param.marker} ${displayName}: ${formatMetric(value[1], metric, currency)}`;
                        tooltipText += '<br/>';
                    });
                    return tooltipText;
                }
            },
            legend: {
                orient: 'vertical',
                right: 10,
                top: 'center',
                data: legendData,
                type: 'scroll',
                formatter: (name) => {
                    if (metric === 'total_cost') {
                        return name.replace(/ \((.*)\)$/, '');
                    }
                    return name;
                },
                textStyle: {
                    width: 180,
                    overflow: 'truncate'
                },
                tooltip: {
                    show: true
                }
            },
            grid: {
                left: '3%',
                right: 230,
                bottom: '10%',
                containLabel: true
            },
            xAxis: {
                type: 'time',
                axisLabel: {
                    formatter: xAxisFormatter
                }
            },
            yAxis: {
                type: 'value',
                name: t(`dashboard.usageStats.metrics.${metric}`),
            },
            series: series.map(s => ({ ...s, smooth: type === 'line' })),
            dataZoom: [
                {
                    type: 'slider',
                    start: 0,
                    end: 100,
                },
                {
                    type: 'inside',
                    start: 0,
                    end: 100,
                }
            ],
            toolbox: {
                feature: {
                    saveAsImage: {}
                }
            }
        };
    };

    return (
        <div class="mt-6 bg-white p-6 rounded-lg shadow-md">
            <div class="flex justify-between items-center mb-4">
                <div class="flex items-baseline space-x-4">
                    <h2 class="text-xl font-semibold text-gray-700">{t('dashboard.usageStats.title')}</h2>
                    <Show when={totalMetricSumText()}>
                        <span class="text-lg font-medium text-gray-600">
                            {t('dashboard.usageStats.total')}: {totalMetricSumText()}
                        </span>
                    </Show>
                </div>
                <div class="flex items-center space-x-2">
                    <Select
                        value={chartTypeOptions().find(o => o.value === chartType())}
                        onChange={(v) => setChartType(v.value)}
                        options={chartTypeOptions()}
                        optionValue="value"
                        optionTextValue="label"
                        class="w-32"
                    />
                    <Select
                        value={metricOptions().find(o => o.value === selectedMetric())}
                        onChange={(v) => setSelectedMetric(v.value)}
                        options={metricOptions()}
                        optionValue="value"
                        optionTextValue="label"
                        class="w-48"
                    />
                    <Select
                        value={timeRangeOptions().find(o => o.value === timeRange())}
                        onChange={(v) => setTimeRange(v.value)}
                        options={timeRangeOptions()}
                        optionValue="value"
                        optionTextValue="label"
                        class="w-48"
                    />
                    <Button
                        onClick={() => refetchUsageStats()}
                        variant="ghost"
                        size="sm"
                        class="border border-gray-300"
                        disabled={usageStats.loading}
                    >
                        {t('common.refresh')}
                    </Button>
                </div>
            </div>
            <Show when={usageStats() || !usageStats.loading} fallback={<p>{t('loading')}</p>}>
                <Show when={!usageStats.error} fallback={<p class="text-red-500">{t('dashboard.errorLoading', { error: usageStats.error?.message || t('unknownError') })}</p>}>
                    <ECharts options={chartOptions} style={{ height: '400px' }} />
                    <Show when={sortedModelSum().length > 0}>
                        <div class="mt-4">
                            <h3 class="text-lg font-semibold text-gray-700 mb-2">{t('dashboard.usageStats.summary.title')}</h3>
                            <div class="max-h-60 overflow-y-auto border border-gray-200 rounded-md">
                                <table class="min-w-full divide-y divide-gray-200">
                                    <thead class="bg-gray-50 sticky top-0 z-10">
                                        <tr>
                                            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                                                {t('dashboard.usageStats.summary.model')}
                                            </th>
                                            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                                                {t(`dashboard.usageStats.metrics.${selectedMetric()}`)}
                                                <Show when={selectedMetric() !== 'total_cost'}>
                                                    <span> ({t('dashboard.usageStats.total')})</span>
                                                </Show>
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody class="bg-white divide-y divide-gray-200">
                                        <For each={sortedModelSum()}>
                                            {(item) => (
                                                <tr>
                                                    <td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                                                        {selectedMetric() === 'total_cost' ? (item as any).modelName : (item as any)[0]}
                                                    </td>
                                                    <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                                                        {selectedMetric() === 'total_cost'
                                                            ? formatMetric((item as any).sum, 'total_cost', (item as any).currency)
                                                            : formatMetric((item as any)[1], selectedMetric())}
                                                    </td>
                                                </tr>
                                            )}
                                        </For>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </Show>
                </Show>
            </Show>
        </div>
    );
}
