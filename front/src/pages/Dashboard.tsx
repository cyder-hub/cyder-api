import { createResource, Show, For, createSignal } from 'solid-js';
import { useI18n } from '../i18n';
import { request } from '../services/api'; // Use request from api service
import * as echarts from 'echarts/core';
import type { EChartsOption } from 'echarts';
import { LineChart } from 'echarts/charts';
import {
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent,
  DataZoomComponent,
  ToolboxComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import ECharts from '../components/ECharts';

echarts.use([
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent,
  DataZoomComponent,
  ToolboxComponent,
  LineChart,
  CanvasRenderer,
]);

interface SystemOverviewStats {
    providers_count: number;
    models_count: number;
    provider_keys_count: number;
}

interface TodayRequestLogStats {
    requests_count: number;
    total_prompt_tokens: number;
    total_completion_tokens: number;
    total_reasoning_tokens: number;
    total_tokens: number;
    total_cost: Record<string, number>; // Currency -> Cost in micro-units
}

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
    total_cost: number;
}

interface UsageStatsPeriod {
    time: number; // Timestamp for the beginning of the period (milliseconds)
    data: UsageStatItem[];
}

type UsageMetric = 'prompt_tokens' | 'completion_tokens' | 'reasoning_tokens' | 'total_tokens' | 'request_count' | 'total_cost';

const fetchSystemOverview = async (): Promise<SystemOverviewStats> => {
    // The 'request' function is expected to handle .json() and return the data payload directly,
    // or throw an error on non-ok responses.
    // It also handles the auth token.
    try {
        const data = await request('/ai/manager/api/system/overview');
        return data || { providers_count: 0, models_count: 0, provider_keys_count: 0 }; // Return default on null/undefined
    } catch (error) {
        console.error("Failed to fetch system overview:", error);
        throw error; // Re-throw to be caught by createResource error handling
    }
};

const fetchTodayLogStats = async (): Promise<TodayRequestLogStats> => {
    try {
        const data = await request('/ai/manager/api/system/today_log_stats');
        return data || { requests_count: 0, total_prompt_tokens: 0, total_completion_tokens: 0, total_reasoning_tokens: 0, total_tokens: 0, total_cost: {} }; // Return default on null/undefined
    } catch (error) {
        console.error("Failed to fetch today's log stats:", error);
        throw error; // Re-throw to be caught by createResource error handling
    }
};

const fetchUsageStats = async (interval: 'month' | 'day' | 'hour'): Promise<UsageStatsPeriod[]> => {
    try {
        const now = new Date();
        let startTime: Date;

        switch (interval) {
            case 'month':
                startTime = new Date(now);
                // Last 12 months, including the current month.
                startTime.setMonth(now.getMonth() - 11);
                startTime.setDate(1);
                startTime.setHours(0, 0, 0, 0);
                break;
            case 'day':
                startTime = new Date(now);
                // Last 30 days, including today.
                startTime.setDate(now.getDate() - 29);
                startTime.setHours(0, 0, 0, 0);
                break;
            case 'hour':
                startTime = new Date(now);
                // Last 48 hours, including the current hour.
                startTime.setHours(now.getHours() - 47);
                startTime.setMinutes(0, 0, 0);
                break;
        }

        const params = new URLSearchParams({
            interval,
            start_time: startTime.getTime().toString(),
            end_time: now.getTime().toString(),
        });
        const data = await request(`/ai/manager/api/system/usage_stats?${params.toString()}`);
        return data || [];
    } catch (error) {
        console.error("Failed to fetch usage stats:", error);
        throw error;
    }
};

export default function Dashboard() {
    const [t] = useI18n();
    const [overviewStats] = createResource(fetchSystemOverview);
    const [todayLogStats] = createResource(fetchTodayLogStats);
    const [interval, setInterval] = createSignal<'month' | 'day' | 'hour'>('hour');
    const [selectedMetric, setSelectedMetric] = createSignal<UsageMetric>('total_tokens');
    const [usageStats] = createResource(interval, fetchUsageStats);

    const chartOptions = (): EChartsOption => {
        const stats = usageStats();
        const currentInterval = interval();
        const metric = selectedMetric();

        if (!stats) {
            return {
                title: { text: t('loading'), left: 'center', top: 'center' }
            };
        }

        const now = new Date();
        const timeBuckets: number[] = [];
        let startTime: Date;

        switch (currentInterval) {
            case 'month':
                startTime = new Date(now);
                startTime.setMonth(now.getMonth() - 11);
                startTime.setDate(1);
                startTime.setHours(0, 0, 0, 0);
                for (let i = 0; i < 12; i++) {
                    const d = new Date(startTime);
                    d.setMonth(startTime.getMonth() + i);
                    timeBuckets.push(d.getTime());
                }
                break;
            case 'day':
                startTime = new Date(now);
                startTime.setDate(now.getDate() - 29);
                startTime.setHours(0, 0, 0, 0);
                for (let i = 0; i < 30; i++) {
                    const d = new Date(startTime);
                    d.setDate(startTime.getDate() + i);
                    timeBuckets.push(d.getTime());
                }
                break;
            case 'hour':
                startTime = new Date(now);
                startTime.setHours(now.getHours() - 47);
                startTime.setMinutes(0, 0, 0);
                for (let i = 0; i < 48; i++) {
                    const d = new Date(startTime);
                    d.setHours(startTime.getHours() + i);
                    timeBuckets.push(d.getTime());
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

        const seriesMap = new Map<string, { name: string; type: 'line'; data: [number, number][] }>();
        for (const period of stats) {
            for (const item of period.data) {
                const seriesName = `${item.provider_key || t('common.notAvailable')}/${item.model_name || t('common.notAvailable')}`;
                if (!seriesMap.has(seriesName)) {
                    seriesMap.set(seriesName, { name: seriesName, type: 'line', data: [] });
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
                    const item = periodData.find(d =>
                        `${d.provider_key || t('common.notAvailable')}/${d.model_name || t('common.notAvailable')}` === seriesName
                    );
                    if (item) {
                        value = metric === 'total_cost' ? item[metric] / 1_000_000_000 : item[metric];
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

        if (seriesMap.size === 0) {
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
            case 'hour':
                xAxisFormatter = '{HH}:{mm}';
                break;
        }

        return {
            tooltip: {
                trigger: 'axis',
                axisPointer: {
                    type: 'cross'
                }
            },
            legend: {
                data: Array.from(seriesMap.keys()),
                top: 'bottom',
                type: 'scroll',
            },
            grid: {
                left: '3%',
                right: '4%',
                bottom: '15%', // Adjust bottom to make space for legend
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
            series: Array.from(seriesMap.values()),
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
        <div class="p-6 bg-gray-100 min-h-screen">
            <h1 class="text-3xl font-bold text-gray-800 mb-8">{t('sidebar.dashboard')}</h1>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
                {/* System Overview Card */}
                <div class="bg-white p-6 rounded-lg shadow-md">
                    <h2 class="text-xl font-semibold text-gray-700 mb-4">{t('dashboard.systemOverview.title')}</h2>
                    <Show when={!overviewStats.loading && overviewStats()} fallback={<p>{t('loading')}</p>}>
                        <Show when={!overviewStats.error} fallback={<p class="text-red-500">{t('dashboard.errorLoading', { error: overviewStats.error?.message || t('unknownError') })}</p>}>
                            <ul class="space-y-2">
                                <li class="flex justify-between">
                                    <span>{t('dashboard.systemOverview.providers')}:</span>
                                    <span class="font-semibold">{overviewStats()?.providers_count}</span>
                                </li>
                                <li class="flex justify-between">
                                    <span>{t('dashboard.systemOverview.models')}:</span>
                                    <span class="font-semibold">{overviewStats()?.models_count}</span>
                                </li>
                                <li class="flex justify-between">
                                    <span>{t('dashboard.systemOverview.providerKeys')}:</span>
                                    <span class="font-semibold">{overviewStats()?.provider_keys_count}</span>
                                </li>
                            </ul>
                        </Show>
                    </Show>
                </div>

                {/* Today's Log Stats Card */}
                <div class="bg-white p-6 rounded-lg shadow-md">
                    <h2 class="text-xl font-semibold text-gray-700 mb-4">{t('dashboard.todayLogStats.title')}</h2>
                    <Show when={!todayLogStats.loading && todayLogStats()} fallback={<p>{t('loading')}</p>}>
                         <Show when={!todayLogStats.error} fallback={<p class="text-red-500">{t('dashboard.errorLoading', { error: todayLogStats.error?.message || t('unknownError') })}</p>}>
                            <ul class="space-y-2">
                                <li class="flex justify-between">
                                    <span>{t('dashboard.todayLogStats.requests')}:</span>
                                    <span class="font-semibold">{todayLogStats()?.requests_count}</span>
                                </li>
                                <li class="flex justify-between">
                                    <span>{t('dashboard.todayLogStats.promptTokens')}:</span>
                                    <span class="font-semibold">{todayLogStats()?.total_prompt_tokens?.toLocaleString()}</span>
                                </li>
                                <li class="flex justify-between">
                                    <span>{t('dashboard.todayLogStats.completionTokens')}:</span>
                                    <span class="font-semibold">{todayLogStats()?.total_completion_tokens?.toLocaleString()}</span>
                                </li>
                                <li class="flex justify-between">
                                    <span>{t('dashboard.todayLogStats.reasoningTokens')}:</span>
                                    <span class="font-semibold">{todayLogStats()?.total_reasoning_tokens?.toLocaleString()}</span>
                                </li>
                                <li class="flex justify-between">
                                    <span>{t('dashboard.todayLogStats.totalTokens')}:</span>
                                    <span class="font-semibold">{todayLogStats()?.total_tokens?.toLocaleString()}</span>
                                </li>
                                <li class="flex justify-between items-start">
                                    <span>{t('dashboard.todayLogStats.totalCost')}:</span>
                                    <div class="text-right">
                                        <For each={Object.entries(todayLogStats()?.total_cost || {})}>
                                            {([currency, cost]) => (
                                                <div class="font-semibold">
                                                    {(cost / 1_000_000_000)} {t(`currencies.${currency}`, {}, currency)}
                                                </div>
                                            )}
                                        </For>
                                    </div>
                                </li>
                            </ul>
                        </Show>
                    </Show>
                </div>
            </div>

            {/* Usage Stats Chart Card */}
            <div class="mt-6 bg-white p-6 rounded-lg shadow-md">
                <div class="flex justify-between items-center mb-4">
                    <h2 class="text-xl font-semibold text-gray-700">{t('dashboard.usageStats.title')}</h2>
                    <div class="flex items-center">
                        <select
                            value={selectedMetric()}
                            onChange={(e) => setSelectedMetric(e.currentTarget.value as UsageMetric)}
                            class="px-3 py-1 border border-gray-300 rounded-md mr-2"
                        >
                            <option value="total_tokens">{t('dashboard.usageStats.metrics.total_tokens')}</option>
                            <option value="prompt_tokens">{t('dashboard.usageStats.metrics.prompt_tokens')}</option>
                            <option value="completion_tokens">{t('dashboard.usageStats.metrics.completion_tokens')}</option>
                            <option value="reasoning_tokens">{t('dashboard.usageStats.metrics.reasoning_tokens')}</option>
                            <option value="request_count">{t('dashboard.usageStats.metrics.request_count')}</option>
                            <option value="total_cost">{t('dashboard.usageStats.metrics.total_cost')}</option>
                        </select>
                        <select
                            value={interval()}
                            onChange={(e) => setInterval(e.currentTarget.value as 'month' | 'day' | 'hour')}
                            class="px-3 py-1 border border-gray-300 rounded-md"
                        >
                            <option value="hour">{t('dashboard.usageStats.intervals.hour')}</option>
                            <option value="day">{t('dashboard.usageStats.intervals.day')}</option>
                            <option value="month">{t('dashboard.usageStats.intervals.month')}</option>
                        </select>
                    </div>
                </div>
                <Show when={!usageStats.loading} fallback={<p>{t('loading')}</p>}>
                    <Show when={!usageStats.error} fallback={<p class="text-red-500">{t('dashboard.errorLoading', { error: usageStats.error?.message || t('unknownError') })}</p>}>
                        <ECharts options={chartOptions} style={{ height: '400px' }} />
                    </Show>
                </Show>
            </div>
        </div>
    );
}
