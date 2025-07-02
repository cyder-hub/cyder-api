import { createSignal, For, Show, createResource } from 'solid-js';
import { Button } from '../components/ui/Button';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '../components/ui/Table';
import { useI18n } from '../i18n'; // Import the i18n hook
import { request } from '../services/api';
import { apiKeys as globalApiKeys, refetchApiKeys as globalRefetchApiKeys } from '../store/apiKeyStore';
import type { ApiKeyItem } from '../store/types';
import ApiKeyEditModal from '../components/ApiKeyEditModal'; // Import the new modal component
import IssueTokenModal from '../components/IssueTokenModal';
import { fetchPoliciesAPI, type AccessControlPolicyFromAPI } from './AccessControlPage';
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
    const [policies] = createResource<AccessControlPolicyFromAPI[]>(fetchPoliciesAPI, { initialValue: [] });
    const [showEditModal, setShowEditModal] = createSignal(false);
    const [showIssueTokenModal, setShowIssueTokenModal] = createSignal(false);
    // This will hold the ApiKeyItem to edit, or null for a new one
    const [selectedApiKey, setSelectedApiKey] = createSignal<ApiKeyItem | null>(null);
    const [apiKeyForToken, setApiKeyForToken] = createSignal<ApiKeyItem | null>(null);
    const [copiedKeyId, setCopiedKeyId] = createSignal<number | null>(null);

    const handleStartEditing = (apiKey?: ApiKeyItem) => {
        setSelectedApiKey(apiKey || null); // Set to null for new, or the item for editing
        setShowEditModal(true);
    };

    const handleStartIssuingToken = (apiKey: ApiKeyItem) => {
        setApiKeyForToken(apiKey);
        setShowIssueTokenModal(true);
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

    const handleCloseIssueTokenModal = () => {
        setShowIssueTokenModal(false);
        setApiKeyForToken(null);
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

    const handleRefreshRef = async (apiKey: ApiKeyItem) => {
        if (confirm(t('apiKeyPage.confirmRefreshRef', { name: apiKey.name }))) {
            try {
                await request(`/ai/manager/api/system_api_key/${apiKey.id}/refresh_ref`, { method: 'POST' });
                globalRefetchApiKeys();
            } catch (error) {
                console.error("Failed to refresh ref for API key:", error);
                alert(t('apiKeyPage.refreshRefFailed', { error: (error as Error).message || t('unknownError') }));
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
                            <For each={globalApiKeys()}>
                                {(key) => (
                                    <TableRow>
                                        <TableCell>{key.name}</TableCell>
                                        <TableCell class="font-mono">
                                            {key.api_key ? `${key.api_key.substring(0, 3)}...${key.api_key.substring(key.api_key.length - 4)}` : 'N/A'}
                                            <Button variant="ghost" size="xs" class="ml-2" onClick={() => copyApiKeyToClipboard(key.api_key, key.id)} title={t('apiKeyPage.copy')}>
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
                                            <Button variant="primary" size="sm" onClick={() => handleStartEditing(key)}>{t('edit')}</Button>
                                            <Button variant="destructive" size="sm" onClick={() => handleDeleteApiKey(key)}>{t('delete')}</Button>
                                            <Button variant="secondary" size="sm" onClick={() => handleRefreshRef(key)}>{t('apiKeyPage.refreshRef')}</Button>
                                            <Button size="sm" onClick={() => handleStartIssuingToken(key)}>{t('apiKeyPage.issueToken')}</Button>
                                        </TableCell>
                                    </TableRow>
                                )}
                            </For>
                        </TableBody>
                    </TableRoot>
                </div>
            </Show>

            {/* Use the new ApiKeyEditModal component */}
            <Show when={policies()}>
                <ApiKeyEditModal
                    isOpen={showEditModal}
                    onClose={handleCloseModal}
                    initialData={selectedApiKey}
                    onSaveSuccess={handleSaveSuccess}
                    policies={policies()!}
                />
            </Show>
            <IssueTokenModal
                isOpen={showIssueTokenModal}
                onClose={handleCloseIssueTokenModal}
                apiKey={apiKeyForToken()}
            />
        </div>
    );
}
