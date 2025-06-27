import { createResource, Show, For } from 'solid-js';
import { useI18n } from '../i18n';
import { request } from '../services/api'; // Use request from api service
import UsageChart from '../components/UsageChart';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/Card';

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

export default function Dashboard() {
    const [t] = useI18n();
    const [overviewStats] = createResource(fetchSystemOverview);
    const [todayLogStats] = createResource(fetchTodayLogStats);

    return (
        <div class="p-6 bg-gray-100 min-h-screen">
            <h1 class="text-3xl font-bold text-gray-800 mb-8">{t('sidebar.dashboard')}</h1>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
                {/* System Overview Card */}
                <Card>
                    <CardHeader>
                        <CardTitle>{t('dashboard.systemOverview.title')}</CardTitle>
                    </CardHeader>
                    <CardContent>
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
                    </CardContent>
                </Card>

                {/* Today's Log Stats Card */}
                <Card>
                    <CardHeader>
                        <CardTitle>{t('dashboard.todayLogStats.title')}</CardTitle>
                    </CardHeader>
                    <CardContent>
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
                    </CardContent>
                </Card>
            </div>

            {/* Usage Stats Chart Card */}
            <UsageChart />
        </div>
    );
}
