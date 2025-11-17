import { createResource, createSignal, createRoot } from 'solid-js';
import { request } from '../services/api';
import type { ProviderListItem } from './types';

const fetchProvidersAPI = async (): Promise<ProviderListItem[]> => {
    try {
        const response = await request("/ai/manager/api/provider/detail/list");
        return response || []; // Assuming `request` returns the data array or null/undefined
    } catch (error) {
        console.error("Failed to fetch global providers:", error);
        return [];
    }
};

function createProviderStore() {
    const [shouldFetch, setShouldFetch] = createSignal(false);

    const [providers, { refetch: refetchProviders }] = createResource<ProviderListItem[]>(shouldFetch, fetchProvidersAPI, { initialValue: [] });

    function loadProviders() {
        setShouldFetch(true);
    }
    return { providers, refetchProviders, loadProviders };
}

const { providers, refetchProviders, loadProviders } = createRoot(createProviderStore);

export { providers, refetchProviders, loadProviders };
