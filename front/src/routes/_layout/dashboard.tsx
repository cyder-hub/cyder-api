import { Show, For, lazy, createResource } from 'solid-js';
import { createFileRoute } from '@tanstack/solid-router';
import { useI18n } from '@/i18n';
import { request } from '@/services/api'; // Use request from api service
const UsageChart = lazy(() => import('@/components/UsageChart'));
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/Card';

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

export const Route = createFileRoute('/_layout/dashboard')({
    component: function Dashboard() {
        const [t] = useI18n();
        const [overview] = createResource(fetchSystemOverview);
        const [todayStats] = createResource(fetchTodayLogStats);

        return (
        <div class="p-6 bg-gray-100 min-h-screen">
            <h1 class="text-3xl font-bold text-gray-800 mb-8">{t('sidebar.dashboard')}</h1>

                <Show when={overview.loading || todayStats.loading}>
                    <p>{t('loading')}</p>
                </Show>
                <Show when={overview.error || todayStats.error}>
                    <p class="text-red-500">{t('dashboard.errorLoading', { error: (overview.error as Error)?.message || (todayStats.error as Error)?.message || t('unknownError') })}</p>
                </Show>
                <Show when={overview() && todayStats()}>
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
                        {/* System Overview Card */}
                        <Card>
                            <CardHeader>
                                <CardTitle>{t('dashboard.systemOverview.title')}</CardTitle>
                            </CardHeader>
                            <CardContent>
                                <ul class="space-y-2">
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.systemOverview.providers')}:</span>
                                        <span class="font-semibold">{overview().providers_count}</span>
                                    </li>
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.systemOverview.models')}:</span>
                                        <span class="font-semibold">{overview().models_count}</span>
                                    </li>
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.systemOverview.providerKeys')}:</span>
                                        <span class="font-semibold">{overview().provider_keys_count}</span>
                                    </li>
                                </ul>
                            </CardContent>
                        </Card>

                        {/* Today's Log Stats Card */}
                        <Card>
                            <CardHeader>
                                <CardTitle>{t('dashboard.todayLogStats.title')}</CardTitle>
                            </CardHeader>
                            <CardContent>
                                <ul class="space-y-2">
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.todayLogStats.requests')}:</span>
                                        <span class="font-semibold">{todayStats().requests_count}</span>
                                    </li>
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.todayLogStats.promptTokens')}:</span>
                                        <span class="font-semibold">{todayStats().total_prompt_tokens?.toLocaleString()}</span>
                                    </li>
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.todayLogStats.completionTokens')}:</span>
                                        <span class="font-semibold">{todayStats().total_completion_tokens?.toLocaleString()}</span>
                                    </li>
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.todayLogStats.reasoningTokens')}:</span>
                                        <span class="font-semibold">{todayStats().total_reasoning_tokens?.toLocaleString()}</span>
                                    </li>
                                    <li class="flex justify-between">
                                        <span>{t('dashboard.todayLogStats.totalTokens')}:</span>
                                        <span class="font-semibold">{todayStats().total_tokens?.toLocaleString()}</span>
                                    </li>
                                    <li class="flex justify-between items-start">
                                        <span>{t('dashboard.todayLogStats.totalCost')}:</span>
                                        <div class="text-right">
                                            <For each={Object.entries(todayStats().total_cost || {})}>
                                                {([currency, cost]) => (
                                                    <div class="font-semibold">
                                                        {(cost / 1_000_000_000)} {t(`currencies.${currency}`, {}, currency)}
                                                    </div>
                                                )}
                                            </For>
                                        </div>
                                    </li>
                                </ul>
                            </CardContent>
                        </Card>
                    </div>

                    {/* Usage Stats Chart Card */}
                    <UsageChart />
                </Show>
        </div>
    );
    }
});
