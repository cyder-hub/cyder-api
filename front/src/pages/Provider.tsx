import { For, Show } from 'solid-js';
import { Button } from '@kobalte/core/button';
import { TextField } from '@kobalte/core/text-field';
// import { Checkbox } from '@kobalte/core/checkbox'; // No longer used directly for provider edit form
import { useNavigate } from '@solidjs/router'; // For navigation
import { request } from '../services/api';
import { providers, refetchProviders as globalRefetchProviders } from '../store/providerStore';
import type { ProviderListItem, ProviderBase, ModelItem, CustomFieldType } from '../store/types'; // Removed types used only in the old modal
import { useI18n } from '../i18n'; // Import useI18n

// --- Type Definitions (specific to this page or not yet moved) ---
// EditingProviderData is moved to ProviderEdit.tsx
// EditableModelItem, ProviderApiKeyItem, CustomFieldItem are moved or not needed here anymore

// ModelItem, ProviderBase, ProviderListItem are now imported from ../store/types

// fetchProviders is now in providerStore.ts
// fetchProviderDetail is moved to ProviderEdit.tsx


export default function Provider() {
    const [t] = useI18n(); // Initialize i18n
    const navigate = useNavigate();
    // Use global providers resource
    // const [providers, { refetch: refetchProviders }] = createResource<ProviderListItem[]>(fetchProviders, { initialValue: [] });
    // showEditModal and editingData are removed as the modal is now a separate page

    // getEmptyProvider is moved to ProviderEdit.tsx

    // handleStartEditing and handleStopEditing are removed

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
                <Button class="btn btn-primary" onClick={() => navigate('/provider/new')}>{t('addProvider')}</Button>
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
                                                        onClick={() => navigate(`/model/edit/${model.model.id}`)}
                                                    >
                                                        {model.model.model_name}
                                                    </span>
                                                )}
                                            </For>
                                        </div>
                                    </div>
                                </div>
                                <div class="p-4 bg-gray-50 border-t border-gray-200 flex justify-end space-x-2">
                                    <Button class="btn btn-primary btn-sm" onClick={() => navigate(`/provider/edit/${item.provider.id}`)}>{t('edit')}</Button>
                                    <Button class="btn btn-danger btn-sm" onClick={() => handleDeleteProvider(item.provider)}>{t('delete')}</Button>
                                </div>
                            </div>
                        )}
                    </For>
                </div>
            </Show>

            {/* Edit Provider Modal has been removed and is now a separate page */}

            {/* Inline styles from provider.html, consider moving to a CSS file or module */}
            <style jsx global>{`
                .clickable {
                    cursor: pointer;
                    text-decoration: underline;
                    color: #3b82f6; /* blue-500 */
                }
                .clickable:hover {
                    color: #1d4ed8; /* blue-700 */
                }
                /* Styles for .section, .section-title, .required-field are moved to ProviderEdit.tsx or global css */
                .model-tag {
                    background-color: #f3f4f6; /* gray-100 */
                    border-radius: 0.25rem; /* rounded-md */
                    padding: 0.25rem 0.5rem; /* py-1 px-2 */
                    margin-right: 0.5rem; /* 8px */
                    margin-bottom: 0.5rem; /* Added for spacing if they wrap */
                    display: inline-block;
                    font-size: 0.875rem; /* text-sm */
                }
                /* Ensure Kobalte Dialog content has a max height and is scrollable */
                .kb-dialog__content { /* Default Kobalte class, or use your own as in the example */
                    max-height: 90vh;
                    display: flex;
                    flex-direction: column;
                }
                .kb-dialog__content > div:first-of-type { /* Assuming first div is the scrollable content area */
                    overflow-y: auto;
                }
                /* General form item styling is moved to ProviderEdit.tsx or global css */
                /* .form-item, .form-label, .form-input, .form-select, .form-checkbox are moved */
                .btn {
                    padding: 0.5rem 1rem;
                    border-radius: 0.375rem;
                    font-weight: 500;
                    transition: background-color 0.15s ease-in-out;
                    box-shadow: 0 1px 2px 0 rgba(0,0,0,0.05);
                }
                .btn-sm { padding: 0.25rem 0.75rem; font-size: 0.875rem; }
                .btn-primary { background-color: #2563eb; color: white; }
                .btn-primary:hover { background-color: #1d4ed8; }
                .btn-secondary { background-color: #6b7280; color: white; }
                .btn-secondary:hover { background-color: #4b5563; }
                .btn-danger { background-color: #dc2626; color: white; }
                .btn-danger:hover { background-color: #b91c1c; }
                .btn-success { background-color: #16a34a; color: white; }
                .btn-success:hover { background-color: #15803d; }

                /* For Kobalte Select Trigger to look like form-input */
                .kb-select__trigger.form-select {
                     /* padding already handled by .form-select */
                }
            `}</style>
        </div>
    );
}
