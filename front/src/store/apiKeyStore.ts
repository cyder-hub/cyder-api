import { createResource, createSignal, createRoot } from 'solid-js';
import { request } from '../services/api';
import type { ApiKeyItem } from './types';

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

export const fetchApiKeysAPI = async (): Promise<ApiKeyItem[]> => {
    try {
        const data = await request<ApiKeyItem[]>("/ai/manager/api/system_api_key/list");
        return (data || []).map(key => ({
            ...key,
            created_at_formatted: formatTimestamp(key.created_at),
            updated_at_formatted: formatTimestamp(key.updated_at),
        }));
    } catch (error) {
        console.error("Failed to fetch API keys:", error);
        return [];
    }
};

function createApiKeyStore() {
    const [shouldFetch, setShouldFetch] = createSignal(false);

    // Eagerly fetch API keys on app startup
    const [apiKeys, { refetch: refetchApiKeys }] = createResource<ApiKeyItem[]>(shouldFetch, fetchApiKeysAPI, { initialValue: [] });

    function loadApiKeys() {
        setShouldFetch(true);
    }
    return { apiKeys, refetchApiKeys, loadApiKeys };
}

const { apiKeys, refetchApiKeys, loadApiKeys } = createRoot(createApiKeyStore);

export { apiKeys, refetchApiKeys, loadApiKeys };
