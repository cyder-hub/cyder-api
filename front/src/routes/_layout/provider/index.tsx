import { For, Show } from 'solid-js';
import { Button } from '@/components/ui/Button';
import { createFileRoute, useNavigate } from '@tanstack/solid-router';
import { request } from '@/services/api';
import { providers, loadProviders, refetchProviders as globalRefetchProviders } from '@/store/providerStore';
import type { ProviderBase } from '@/store/types';
import { useI18n } from '@/i18n';

function ProviderPage() {
    loadProviders();

    const [t] = useI18n();
    const navigate = useNavigate({ from: Route.fullPath });

    const handleDeleteProvider = async (provider: ProviderBase) => {
        if (confirm(t('confirmDeleteProvider', { name: provider.name }))) {
            try {
                await request(`/ai/manager/api/provider/${provider.id}`, { method: 'DELETE' });
                globalRefetchProviders();
            } catch (error) {
                console.error("Failed to delete provider:", error);
                alert(t('deleteFailed', { error: (error as Error).message || t('unknownError') }));
            }
        }
    };

    // handleCommitProvider is moved to ProviderEdit.tsx

    // Dynamic list item updates for EditingData (updateEditingData, addListItem, removeListItem, updateListItem) are moved to ProviderEdit.tsx
    // providerTypes and customFieldTypes are moved to ProviderEdit.tsx


    return (
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-semibold mb-4 text-gray-800">{t('providerPageTitle')}</h1>

            <div class="mb-4">
                <Button variant="primary" onClick={() => navigate({ to: '/provider/new' })}>{t('addProvider')}</Button>
            </div>

            {/* Data Table */}
            <Show when={providers.loading}>
                <div class="text-center py-4 text-gray-500">{t('providersLoading')}</div>
            </Show>
            <Show when={!providers.loading && providers.error}>
                <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                    {t('providersError', { error: providers.error instanceof Error ? providers.error.message : t('unknownError') })}
                </div>
            </Show>
            <Show when={!providers.loading && !providers.error && providers()?.length === 0}>
                 <div class="text-center py-4 text-gray-500">{t('noProviders')}</div>
            </Show>

            <Show when={!providers.loading && !providers.error && providers() && providers()!.length > 0}>
                <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
                    <For each={providers()}>
                        {(item) => (
                            <div class="bg-white rounded-lg shadow-md border border-gray-200 flex flex-col">
                                <div class="p-4 border-b border-gray-200">
                                    <h3 class="text-lg font-semibold text-gray-800">{item.provider.name}</h3>
                                    <p class="text-sm text-gray-500">{item.provider.provider_key}</p>
                                </div>
                                <div class="p-4 space-y-3 flex-grow">
                                    <div class="flex justify-between items-center">
                                        <span class="text-sm font-medium text-gray-600">{t('tableHeaderType')}:</span>
                                        <span class="text-sm text-gray-800">{item.provider.provider_type}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-sm font-medium text-gray-600">{t('tableHeaderUseProxy')}:</span>
                                        <span class={`px-2 inline-flex text-xs leading-5 font-semibold rounded-full ${item.provider.use_proxy ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'}`}>
                                            {item.provider.use_proxy ? t('common.yes') : t('common.no')}
                                        </span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-sm font-medium text-gray-600">{t('providerApiKeys')}:</span>
                                        <span class="text-sm text-gray-800">{item.provider_keys.length}</span>
                                    </div>
                                    <div>
                                        <span class="text-sm font-medium text-gray-600 mb-1 block">{t('providerModels')}:</span>
                                        <div class="flex flex-wrap gap-2">
                                            <For each={item.models}>
                                                {(model) => (
                                                    <span
                                                        class="model-tag clickable"
                                                        title={t('providerPage.editModel', { model_name: model.model.model_name })}
                                                        onClick={() => navigate({ to: `/model/edit/${model.model.id}` })}
                                                    >
                                                        {model.model.model_name}
                                                    </span>
                                                )}
                                            </For>
                                        </div>
                                    </div>
                                </div>
                                <div class="p-4 bg-gray-50 border-t border-gray-200 flex justify-end space-x-2">
                                    <Button variant="primary" size="sm" onClick={() => navigate({ to: `/provider/edit/${item.provider.id}` })}>{t('edit')}</Button>
                                    <Button variant="destructive" size="sm" onClick={() => handleDeleteProvider(item.provider)}>{t('delete')}</Button>
                                </div>
                            </div>
                        )}
                    </For>
                </div>
            </Show>

            {/* Edit Provider Modal has been removed and is now a separate page */}

        </div>
    );
}

export const Route = createFileRoute('/_layout/provider/')({
    component: ProviderPage,
});
