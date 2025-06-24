import { createSignal, For, Show } from 'solid-js';
import { Button } from '@kobalte/core/button';
import { useI18n } from '../i18n'; // Import the i18n hook
import { request } from '../services/api';
import { apiKeys as globalApiKeys, refetchApiKeys as globalRefetchApiKeys } from '../store/apiKeyStore';
import type { ApiKeyItem } from '../store/types';
import ApiKeyEditModal from '../components/ApiKeyEditModal'; // Import the new modal component
// EditingApiKeyData interface is now in ApiKeyEditModal.tsx

const formatTimestamp = (ms: number | undefined | null): string => {
    if (!ms) return '';
    const date = new Date(ms);
    const YYYY = date.getFullYear();
    const MM = String(date.getMonth() + 1).padStart(2, '0');
    const DD = String(date.getDate()).padStart(2, '0');
    const hh = String(date.getHours()).padStart(2, '0');
    const mm = String(date.getMinutes()).padStart(2, '0');
    const ss = String(date.getSeconds()).padStart(2, '0');
    return `${YYYY}-${MM}-${DD} ${hh}:${mm}:${ss}`;
};

// fetchApiKeys is now in apiKeyStore.ts

export default function ApiKeyPage() {
    const [t] = useI18n(); // Initialize the t function
    const [showEditModal, setShowEditModal] = createSignal(false);
    // This will hold the ApiKeyItem to edit, or null for a new one
    const [selectedApiKey, setSelectedApiKey] = createSignal<ApiKeyItem | null>(null);
    const [copiedKeyId, setCopiedKeyId] = createSignal<number | null>(null);

    const handleStartEditing = (apiKey?: ApiKeyItem) => {
        setSelectedApiKey(apiKey || null); // Set to null for new, or the item for editing
        setShowEditModal(true);
    };

    const handleToggleEnable = async (apiKey: ApiKeyItem) => {
        const updatedApiKey = { ...apiKey, is_enabled: !apiKey.is_enabled };
        try {
            // Prepare payload, similar to what ApiKeyEditModal would send for an update
            // The backend expects the full key data for an update.
            const payload = {
                name: updatedApiKey.name,
                api_key: updatedApiKey.api_key, // Send existing key, backend might ignore or re-validate
                description: updatedApiKey.description,
                is_enabled: updatedApiKey.is_enabled,
            };
            await request(`/ai/manager/api/system_api_key/${updatedApiKey.id}`, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload),
            });
            globalRefetchApiKeys();
        } catch (error) {
            console.error("Failed to toggle API key status:", error);
            alert(t('apiKeyPage.toggleStatusFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
            // Optionally, refetch to revert optimistic UI changes if any were made,
            // or revert the checkbox state manually if it was optimistically updated.
            // For now, a refetch will correct the UI.
            globalRefetchApiKeys();
        }
    };

    const handleCloseModal = () => {
        setShowEditModal(false);
        setSelectedApiKey(null); // Clear selected data when modal closes
    };

    const handleSaveSuccess = () => {
        globalRefetchApiKeys();
        // The modal will call its own onClose, but we ensure state is clean here too
        handleCloseModal();
    };

    const handleDeleteApiKey = async (apiKey: ApiKeyItem) => {
        if (confirm(t('apiKeyPage.confirmDelete', { name: apiKey.name }))) {
            try {
                await request(`/ai/manager/api/system_api_key/${apiKey.id}`, { method: 'DELETE' });
                globalRefetchApiKeys();
            } catch (error) {
                console.error("Failed to delete API key:", error);
                alert(t('deleteFailed', { error: (error as Error).message || t('unknownError') }));
            }
        }
    };

    const copyApiKeyToClipboard = (apiKeyString: string, keyId: number) => {
        if (!apiKeyString) return;
        const textArea = document.createElement("textarea");
        textArea.value = apiKeyString;
        
        // Prevent scrolling to bottom of page in MS Edge.
        textArea.style.top = "0";
        textArea.style.left = "0";
        textArea.style.position = "fixed";

        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();

        try {
            const successful = document.execCommand('copy');
            if (successful) {
                setCopiedKeyId(keyId);
                setTimeout(() => setCopiedKeyId(null), 2000); // Reset after 2 seconds
            } else {
                console.error('Failed to copy API key using execCommand.');
                alert(t('apiKeyPage.copyFailed'));
            }
        } catch (err) {
            console.error('Failed to copy API key: ', err);
            alert(t('apiKeyPage.copyFailed'));
        }

        document.body.removeChild(textArea);
    };


    return (
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-semibold mb-4 text-gray-800">{t('apiKeyPage.title')}</h1>

            <div class="mb-4">
                <Button class="btn btn-primary" onClick={() => handleStartEditing()}>{t('apiKeyPage.addApiKey')}</Button>
            </div>

            {/* Data Table */}
            <Show when={globalApiKeys.loading}>
                <div class="text-center py-4 text-gray-500">{t('apiKeyPage.loading')}</div>
            </Show>
            <Show when={!globalApiKeys.loading && globalApiKeys.error}>
                <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                    {t('apiKeyPage.errorPrefix')} {globalApiKeys.error instanceof Error ? globalApiKeys.error.message : t('unknownError')}
                </div>
            </Show>
            <Show when={!globalApiKeys.loading && !globalApiKeys.error && globalApiKeys()?.length === 0}>
                 <div class="text-center py-4 text-gray-500">{t('apiKeyPage.noData')}</div>
            </Show>

            <Show when={!globalApiKeys.loading && !globalApiKeys.error && globalApiKeys() && globalApiKeys()!.length > 0}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <table class="min-w-full divide-y divide-gray-200 data-table">
                        <thead class="bg-gray-100">
                            <tr>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.name')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.apiKeyPartial')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.description')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.enabled')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.createdAt')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.updatedAt')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('apiKeyPage.table.actions')}</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">
                            <For each={globalApiKeys()}>
                                {(key) => (
                                    <tr class="hover:bg-gray-50">
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800">{key.name}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800 font-mono">
                                            {key.api_key ? `${key.api_key.substring(0, 3)}...${key.api_key.substring(key.api_key.length - 4)}` : 'N/A'}
                                            <Button class="btn btn-xs btn-ghost ml-2" onClick={() => copyApiKeyToClipboard(key.api_key, key.id)} title={t('apiKeyPage.copy')}>
                                                {copiedKeyId() === key.id ? t('apiKeyPage.copied') : t('apiKeyPage.copy')}
                                            </Button>
                                        </td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 max-w-xs truncate" title={key.description}>{key.description || '/'}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">
                                            <input
                                                type="checkbox"
                                                class="form-checkbox" // Use existing inline style for form-checkbox
                                                checked={key.is_enabled}
                                                onChange={() => handleToggleEnable(key)}
                                                // id={`enable-apikey-${key.id}`} // Optional: for label association if a label were present
                                            />
                                        </td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{key.created_at_formatted}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{key.updated_at_formatted}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm space-x-2">
                                            <Button class="btn btn-primary btn-sm" onClick={() => handleStartEditing(key)}>{t('edit')}</Button>
                                            <Button class="btn btn-danger btn-sm" onClick={() => handleDeleteApiKey(key)}>{t('delete')}</Button>
                                        </td>
                                    </tr>
                                )}
                            </For>
                        </tbody>
                    </table>
                </div>
            </Show>

            {/* Use the new ApiKeyEditModal component */}
            <ApiKeyEditModal
                isOpen={showEditModal}
                onClose={handleCloseModal}
                initialData={selectedApiKey}
                onSaveSuccess={handleSaveSuccess}
            />
            
            {/* Styles (can be moved to a global CSS file or CSS module) */}
            <style jsx global>{`
                /* Add any specific styles for ApiKeyPage here if needed, or use global styles */
                .form-item { margin-bottom: 1rem; }
                .form-label { display: block; margin-bottom: 0.25rem; font-weight: 500; color: #374151; }
                .form-input, .form-select {
                    width: 100%;
                    padding: 0.5rem 0.75rem;
                    border: 1px solid #d1d5db;
                    border-radius: 0.375rem;
                    box-shadow: inset 0 1px 2px 0 rgba(0,0,0,0.05);
                }
                .form-input:focus, .form-select:focus {
                    border-color: #2563eb;
                    outline: 2px solid transparent;
                    outline-offset: 2px;
                    box-shadow: 0 0 0 2px #bfdbfe;
                }
                .form-checkbox {
                    border-radius: 0.25rem;
                    border-color: #d1d5db;
                }
                .form-checkbox:focus {
                     border-color: #2563eb;
                     box-shadow: 0 0 0 2px #bfdbfe;
                }
                .btn {
                    padding: 0.5rem 1rem;
                    border-radius: 0.375rem;
                    font-weight: 500;
                    transition: background-color 0.15s ease-in-out;
                    box-shadow: 0 1px 2px 0 rgba(0,0,0,0.05);
                }
                .btn-sm { padding: 0.25rem 0.75rem; font-size: 0.875rem; }
                .btn-xs { padding: 0.125rem 0.5rem; font-size: 0.75rem; }
                .btn-primary { background-color: #2563eb; color: white; }
                .btn-primary:hover { background-color: #1d4ed8; }
                .btn-secondary { background-color: #6b7280; color: white; }
                .btn-secondary:hover { background-color: #4b5563; }
                .btn-danger { background-color: #dc2626; color: white; }
                .btn-danger:hover { background-color: #b91c1c; }
                .btn-ghost { background-color: transparent; border-color: transparent; color: #2563eb; }
                .btn-ghost:hover { background-color: #eff6ff; }
                .data-table { table-layout: fixed; } /* Helps with column widths and truncation */
                .max-w-xs { max-width: 20rem; } /* Example for description column */
                .truncate {
                    overflow: hidden;
                    text-overflow: ellipsis;
                    white-space: nowrap;
                }
            `}</style>
        </div>
    );
}
