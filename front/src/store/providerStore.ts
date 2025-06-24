import { createResource } from 'solid-js';
import { request } from '../services/api';
import type { ProviderListItem } from './types';

const fetchProviders = async (): Promise<ProviderListItem[]> => {
    try {
        const response = await request("/ai/manager/api/provider/detail/list");
        return response || []; // Assuming `request` returns the data array or null/undefined
    } catch (error) {
        console.error("Failed to fetch global providers:", error);
        return [];
    }
};

const [providers, { refetch: refetchProviders }] = createResource<ProviderListItem[]>(fetchProviders, { initialValue: [] });

export { providers, refetchProviders };
