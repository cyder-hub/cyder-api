import { createSignal, For, Show, onMount } from 'solid-js';
import { createFileRoute, useRouter } from '@tanstack/solid-router';
import { Button } from '@/components/ui/Button';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '@/components/ui/Table';
import { useI18n } from '@/i18n'; // Import the i18n hook
import { request } from '@/services/api';
import type { ApiKeyItem } from '@/store/types';
import ApiKeyEditModal from '@/components/ApiKeyEditModal'; // Import the new modal component
import { policies, loadPolicies } from '@/store/accessControlStore.ts';
import { apiKeys, refetchApiKeys, loadApiKeys } from '@/store/apiKeyStore.ts';
// EditingApiKeyData interface is now in ApiKeyEditModal.tsx

export const Route = createFileRoute('/_layout/api_key')({
    component: ApiKeyPage,
});

export default function ApiKeyPage() {
    onMount(async () => {
        await Promise.all([
            loadApiKeys(),
            loadPolicies()
        ]);
    });

    const [t] = useI18n(); // Initialize the t function
    const router = useRouter();
    const [showEditModal, setShowEditModal] = createSignal(false);
    // This will hold the ApiKeyItem to edit, or null for a new one
    const [selectedApiKey, setSelectedApiKey] = createSignal<ApiKeyItem | null>(null);
    const [apiKeyForToken, setApiKeyForToken] = createSignal<ApiKeyItem | null>(null);
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
                access_control_policy_id: (updatedApiKey as any).access_control_policy_id,
            };
            await request(`/ai/manager/api/system_api_key/${updatedApiKey.id}`, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload),
            });
            router.invalidate();
        } catch (error) {
            console.error("Failed to toggle API key status:", error);
            alert(t('apiKeyPage.toggleStatusFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
            // Optionally, refetch to revert optimistic UI changes if any were made,
            // or revert the checkbox state manually if it was optimistically updated.
            // For now, a refetch will correct the UI.
            router.invalidate();
        }
    };

    const handleCloseModal = () => {
        setShowEditModal(false);
        setSelectedApiKey(null); // Clear selected data when modal closes
    };

    const handleSaveSuccess = () => {
        router.invalidate();
        // The modal will call its own onClose, but we ensure state is clean here too
        handleCloseModal();
    };

    const handleDeleteApiKey = async (apiKey: ApiKeyItem) => {
        if (confirm(t('apiKeyPage.confirmDelete', { name: apiKey.name }))) {
            try {
                await request(`/ai/manager/api/system_api_key/${apiKey.id}`, { method: 'DELETE' });
                router.invalidate();
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
                <Button variant="primary" onClick={() => handleStartEditing()}>{t('apiKeyPage.addApiKey')}</Button>
            </div>

            {/* Data Table */}
            <Show when={apiKeys.loading}>
                <div class="text-center py-4 text-gray-500">{t('apiKeyPage.loading')}</div>
            </Show>
            <Show when={!apiKeys.loading && apiKeys.error}>
                <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                    {t('apiKeyPage.errorPrefix')} {apiKeys.error instanceof Error ? apiKeys.error.message : t('unknownError')}
                </div>
            </Show>
            <Show when={!apiKeys.loading && !apiKeys.error && apiKeys()?.length === 0}>
                 <div class="text-center py-4 text-gray-500">{t('apiKeyPage.noData')}</div>
            </Show>

            <Show when={!apiKeys.loading && !apiKeys.error && apiKeys() && apiKeys()!.length > 0}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <TableRoot>
                        <TableHeader>
                            <TableRow>
                                <TableColumnHeader>{t('apiKeyPage.table.name')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.apiKeyPartial')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.description')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.enabled')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.accessControlPolicy')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.createdAt')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.updatedAt')}</TableColumnHeader>
                                <TableColumnHeader>{t('apiKeyPage.table.actions')}</TableColumnHeader>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            <For each={apiKeys()}>
                                {(key) => (
                                    <TableRow>
                                        <TableCell>{key.name}</TableCell>
                                        <TableCell class="font-mono">
                                            {key.api_key ? `${key.api_key.substring(0, 3)}...${key.api_key.substring(key.api_key.length - 4)}` : 'N/A'}
                                            <Button type="text" variant="ghost" size="xs" class="ml-2" onClick={() => copyApiKeyToClipboard(key.api_key, key.id)} title={t('apiKeyPage.copy')}>
                                                {copiedKeyId() === key.id ? t('apiKeyPage.copied') : t('apiKeyPage.copy')}
                                            </Button>
                                        </TableCell>
                                        <TableCell class="max-w-xs truncate" title={key.description}>{key.description || '/'}</TableCell>
                                        <TableCell>
                                            <input
                                                type="checkbox"
                                                class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                                                checked={key.is_enabled}
                                                onChange={() => handleToggleEnable(key)}
                                            />
                                        </TableCell>
                                        <TableCell>{(key as any).access_control_policy_name || t('common.notAvailable')}</TableCell>
                                        <TableCell>{key.created_at_formatted}</TableCell>
                                        <TableCell>{key.updated_at_formatted}</TableCell>
                                        <TableCell class="space-x-2">
                                            <Button type="text" variant="primary" size="sm" onClick={() => handleStartEditing(key)}>{t('edit')}</Button>
                                            <Button type="text" variant="destructive" size="sm" onClick={() => handleDeleteApiKey(key)}>{t('delete')}</Button>
                                        </TableCell>
                                    </TableRow>
                                )}
                            </For>
                        </TableBody>
                    </TableRoot>
                </div>
            </Show>

            {/* Use the new ApiKeyEditModal component */}
            <ApiKeyEditModal
                isOpen={showEditModal}
                onClose={handleCloseModal}
                initialData={selectedApiKey}
                onSaveSuccess={handleSaveSuccess}
            />
        </div>
    );
}
