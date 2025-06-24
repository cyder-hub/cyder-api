import { createResource } from 'solid-js';
import { request } from '../services/api';
import type { ApiKeyItem } from './types'; // Import the shared type

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

const fetchApiKeysGlobal = async (): Promise<ApiKeyItem[]> => {
    try {
        const responseData = await request("/ai/manager/api/system_api_key/list");
        const keys: ApiKeyItem[] = Array.isArray(responseData) ? responseData : [];
        return keys.map(key => ({
            ...key,
            created_at_formatted: formatTimestamp(key.created_at),
            updated_at_formatted: formatTimestamp(key.updated_at),
        }));
    } catch (error) {
        console.error("Failed to fetch global API keys:", error);
        return [];
    }
};

const [apiKeys, { refetch: refetchApiKeys }] = createResource<ApiKeyItem[]>(fetchApiKeysGlobal, { initialValue: [] });

export { apiKeys, refetchApiKeys };
