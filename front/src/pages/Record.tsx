import { createSignal, createEffect, For, Show, createResource, Accessor, Setter, createMemo } from 'solid-js';
import type { Resource } from 'solid-js';
import { useI18n } from '../i18n'; // Import the i18n hook
import { Button } from '@kobalte/core/button';
import { Popover } from '@kobalte/core/popover'; // Import Popover
import { Pagination } from '@kobalte/core/pagination'; // Import Pagination
import { Select } from '@kobalte/core/select';
import { TextField } from '@kobalte/core/text-field';
import { request } from '../services/api'; // Import the centralized request function
import styles from './Record.module.css';

import { providers as globalProviders } from '../store/providerStore'; // Import global providers
import { apiKeys as globalApiKeys } from '../store/apiKeyStore'; // Import global API keys
import type { ProviderListItem, ProviderBase, ApiKeyItem as GlobalApiKeyItem } from '../store/types'; // Import shared types, rename ApiKeyItem to avoid conflict

// --- Type Definitions ---
// ApiKey interface can be removed or replaced by GlobalApiKeyItem if suitable
// interface ApiKey {
// id: number;
// name: string;
// }

// Provider interface can be removed or replaced by ProviderBase if suitable
// interface Provider {
// id: number;
// name: string;
// }

interface RecordItem {
    id: number;
    model_name: string | null;
    provider_id: number | null;
    system_api_key_id: number | null;
    status: string | null;
    prompt_tokens: number | null;
    completion_tokens: number | null;
    reasoning_tokens: number | null;
    total_tokens: number | null;
    is_stream: boolean;
    request_received_at: number | null;
    llm_request_sent_at: number | null;
    llm_response_first_chunk_at: number | null;
    llm_response_completed_at: number | null;
    calculated_cost: number | null;
    cost_currency: string | null;
    request_at_formatted?: string;
    // Display-ready fields
    providerName: string;
    apiKeyName: string;
    isStreamDisplay: string;
    firstRespTimeDisplay: string;
    totalRespTimeDisplay: string;
    tpsDisplay: string;
    costDisplay: string;
}

// Interface for the detailed record fetched on demand
// This should ideally mirror the full RequestLog structure from the backend for the popover
interface RecordDetail extends RecordItem {
    // Assuming the backend returns the full object, which might have more fields
    // For now, we'll just stringify whatever is returned.
    // If specific fields like 'request_body' or 'response_body' are known,
    // they could be typed here.
    [key: string]: any; // Allow any other properties
}

interface Filters {
    api_key_id: number;
    provider_id: number;
    model_name: string;
}

// This interface now represents the actual values passed to fetchRecords
interface FetchRecordsParams {
    page: number;
    size: number;
    currentFilters: Filters;
}

interface FetchRecordsResult {
    list: RecordItem[];
    page: number;
    page_size: number;
    total: number;
}

// fetchApiKeys is removed, will use globalApiKeys from apiKeyStore

// fetchProviders is removed, will use globalProviders from providerStore

// --- Component ---
export default function Record() {
    const [t] = useI18n(); // Initialize the t function
    const [currentPage, setCurrentPage] = createSignal(1);
    const [expandedRecordId, setExpandedRecordId] = createSignal<number | null>(null);

    const initialPageSize = () => {
        const storedSize = localStorage.getItem('pageSize');
        const size = storedSize ? parseInt(storedSize, 10) : 10;
        return !isNaN(size) && size > 0 ? size : 10; // Default to 10 if invalid
    };
    const [pageSize, setPageSize] = createSignal(initialPageSize());

    // currentPage is already defined, will be used by Kobalte Pagination
    // totalPages is already defined, will be used by Kobalte Pagination

    const [modelNameInput, setModelNameInput] = createSignal(''); // For debounced input
    let debounceTimer: number;

    const [filters, setFilters] = createSignal<Filters>({
        api_key_id: 0,
        provider_id: 0,
        model_name: ''
    });

    // Fetch static data
    // const [apiKeys] = createResource<ApiKey[]>(fetchApiKeys, { initialValue: [] }); // Use globalApiKeys instead
    // const [providers] = createResource<Provider[]>(fetchProviders, { initialValue: [] }); // Use globalProviders instead
    console.log("globalApiKeys", globalApiKeys());

    // Fetch records based on reactive parameters
    // recordParams now returns the actual values, making it a proper source for createResource
    const recordParams = () => ({
        page: currentPage(),
        size: pageSize(),
        currentFilters: filters()
    });

    // Fetch detailed record when expandedRecordId changes
    const fetchRecordDetailsById = async (recordId: number | null): Promise<RecordDetail | null> => {
        if (recordId === null) {
            return null;
        }
        try {
            // Assuming the API returns HttpResult<RequestLog> which has a 'data' field
            // or the result itself is the RequestLog object.
            // Adjust based on actual API response structure.
            const response = await request(`/ai/manager/api/request_log/${recordId}`);
            // If response is wrapped in a 'data' object by HttpResult
            // return (response as any).data || response; 
            return response; // Assuming 'request' unwraps HttpResult or returns the core data
        } catch (error) {
            console.error(`Failed to fetch details for record ${recordId}:`, error);
            throw error; // Propagate error to resource
        }
    };
    const [detailedRecordData] = createResource(expandedRecordId, fetchRecordDetailsById);

    const totalPages = () => {
        const totalItems = recordsResult()?.total ?? 0;
        const size = pageSize();
        return size > 0 ? Math.ceil(totalItems / size) : 0;
    };

    // Memoized maps for lookups
    const providerMap = () => {
        const map = new Map<string | number, string>();
        globalProviders()?.forEach(item => map.set(item.provider.id, item.provider.name));
        return map;
    };
    const apiKeyMap = () => {
        const map = new Map<string | number, string>();
        globalApiKeys()?.forEach(k => map.set(k.id, k.name));
        return map;
    };

    const apiKeysForSelect = createMemo(() => {
        const allKey: GlobalApiKeyItem = {
            id: 0,
            name: t('recordPage.filter.allApiKeys'),
            api_key: '',
            description: '',
            enabled: false,
            created_at: '',
            updated_at: '',
        };
        return [allKey, ...(globalApiKeys() || [])];
    });

    const providersForSelect = createMemo(() => {
        const allProviderItem: ProviderListItem = {
            provider: {
                id: 0,
                name: t('recordPage.filter.allProviders'),
                provider_key: '',
                endpoint: '',
                use_proxy: false,
                provider_type: '',
            }
        };
        return [allProviderItem, ...(globalProviders() || [])];
    });

    const fetchRecords = async ({ page, size, currentFilters }: FetchRecordsParams): Promise<FetchRecordsResult> => {
        try {
            let queryParams = `page=${page}&page_size=${size}`;
            // Use currentFilters directly
            if (currentFilters.provider_id) {
                queryParams += `&provider_id=${encodeURIComponent(currentFilters.provider_id)}`;
            }
            if (currentFilters.model_name) {
                queryParams += `&model_name=${encodeURIComponent(currentFilters.model_name)}`;
            }
            if (currentFilters.api_key_id) {
                queryParams += `&system_api_key_id=${encodeURIComponent(currentFilters.api_key_id)}`;
            }
            // Assuming request returns an object matching FetchRecordsResult structure
            const result: any = await request(`/ai/manager/api/request_log/list?${queryParams}`);

            // Format date and ensure list is an array
            const list: RecordItem[] = (result?.list || []).map((backendRecord: any) => {
                let request_at_formatted = '/';
                if (backendRecord.request_received_at) {
                    try {
                        const date = new Date(backendRecord.request_received_at);
                        if (!isNaN(date.getTime())) {
                            const year = date.getFullYear();
                            const month = String(date.getMonth() + 1).padStart(2, '0');
                            const day = String(date.getDate()).padStart(2, '0');
                            const hours = String(date.getHours()).padStart(2, '0');
                            const minutes = String(date.getMinutes()).padStart(2, '0');
                            const seconds = String(date.getSeconds()).padStart(2, '0');
                            request_at_formatted = `${year}-${month}-${day} ${hours}:${minutes}:${seconds}`;
                        }
                    } catch (e) {
                        console.error("Error formatting date:", e);
                    }
                }
                const status = backendRecord.status || null;

                // New calculations for display
                const providerName = backendRecord.provider_id != null ? providerMap().get(backendRecord.provider_id) || '/' : '/';
                const apiKeyName = backendRecord.system_api_key_id != null ? apiKeyMap().get(backendRecord.system_api_key_id) || '/' : '/';
                const isStreamDisplay = backendRecord.is_stream ? t('common.yes') : t('common.no');
                const firstRespTimeDisplay = (backendRecord.llm_response_first_chunk_at != null && backendRecord.llm_request_sent_at != null) ? ((backendRecord.llm_response_first_chunk_at - backendRecord.llm_request_sent_at) / 1000).toFixed(3) : '/';
                const totalRespTimeDisplay = (backendRecord.llm_response_completed_at != null && backendRecord.llm_request_sent_at != null) ? ((backendRecord.llm_response_completed_at - backendRecord.llm_request_sent_at) / 1000).toFixed(3) : '/';

                let tpsDisplay = '/';
                if (backendRecord.completion_tokens != null && backendRecord.llm_response_completed_at != null) {
                    let durationMs;
                    if (backendRecord.is_stream) {
                        if (backendRecord.llm_response_first_chunk_at != null) {
                            durationMs = backendRecord.llm_response_completed_at - backendRecord.llm_response_first_chunk_at;
                        }
                    } else {
                        if (backendRecord.llm_request_sent_at != null) {
                            durationMs = backendRecord.llm_response_completed_at - backendRecord.llm_request_sent_at;
                        }
                    }
                    if (durationMs != null && durationMs > 0) {
                        tpsDisplay = (backendRecord.completion_tokens / (durationMs / 1000)).toFixed(2);
                    }
                }

                const costDisplay = backendRecord.calculated_cost != null ? `${backendRecord.cost_currency || ''} ${(backendRecord.calculated_cost / 1000000000)}` : '/';

                // Ensure the returned object matches RecordItem structure
                return {
                    id: backendRecord.id,
                    model_name: backendRecord.model_name || null,
                    provider_id: backendRecord.provider_id ?? null,
                    system_api_key_id: backendRecord.system_api_key_id ?? null,
                    status: status,
                    prompt_tokens: backendRecord.prompt_tokens ?? null,
                    completion_tokens: backendRecord.completion_tokens ?? null,
                    reasoning_tokens: backendRecord.reasoning_tokens ?? null,
                    total_tokens: backendRecord.total_tokens ?? null,
                    is_stream: backendRecord.is_stream ?? false,
                    request_received_at: backendRecord.request_received_at ?? null,
                    llm_request_sent_at: backendRecord.llm_request_sent_at ?? null,
                    llm_response_first_chunk_at: backendRecord.llm_response_first_chunk_at ?? null,
                    llm_response_completed_at: backendRecord.llm_response_completed_at ?? null,
                    calculated_cost: backendRecord.calculated_cost ?? null,
                    cost_currency: backendRecord.cost_currency || null,
                    request_at_formatted,
                    // Display fields
                    providerName,
                    apiKeyName,
                    isStreamDisplay,
                    firstRespTimeDisplay,
                    totalRespTimeDisplay,
                    tpsDisplay,
                    costDisplay,
                };
            });

            // Ensure result structure is consistent
            return {
                list: list,
                page: result?.page || 1,
                page_size: result?.page_size || size,
                total: result?.total || 0
            };
        } catch (error) {
            console.error("Failed to fetch records:", error);
            return { list: [], page: page, page_size: size, total: 0 };
        }
    };

    const [recordsResult] = createResource<FetchRecordsResult, FetchRecordsParams>(recordParams, fetchRecords);

    const applyFilter = () => {
        setCurrentPage(1);
    };

    const resetFilter = () => {
        setModelNameInput(''); // Clear debounced input as well
        setFilters({ api_key_id: 0, provider_id: 0, model_name: '' });
        setCurrentPage(1);
    };

    const handlePageSizeChange = (event: Event) => {
        const target = event.target as HTMLSelectElement; // Type assertion
        const newSize = parseInt(target.value, 10);
        if (!isNaN(newSize) && newSize > 0) {
            localStorage.setItem('pageSize', String(newSize));
            setPageSize(newSize);
            setCurrentPage(1);
        }
    };

    // nextPage, previousPage, goToPage, and getVisiblePages are no longer needed
    // as Kobalte's Pagination component will handle this.

    // Effect for debouncing model name filter
    createEffect(() => {
        const currentModelName = modelNameInput();
        clearTimeout(debounceTimer);
        debounceTimer = window.setTimeout(() => {
            setFilters(f => ({ ...f, model_name: currentModelName }));
            // No need to call applyFilter() here as createResource will react to filters() changing
        }, 600); // 1 second delay
    });

    return (
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-semibold mb-4 text-gray-800">{t('recordPage.title')}</h1>

            {/* Filter Controls */}
            <div class="flex flex-wrap items-center gap-4 p-4 bg-gray-50 rounded-lg shadow-sm border border-gray-200">
                {/* API Key Filter - Use Select instead of Select.Root */}
                <Select<GlobalApiKeyItem> // Use GlobalApiKeyItem
                    value={apiKeysForSelect().find(k => k.id === filters().api_key_id)}
                    onChange={(selectedItem) => setFilters(f => ({ ...f, api_key_id: selectedItem!.id }))}
                    options={apiKeysForSelect()}
                    optionValue="id"
                    optionTextValue="name"
                    disallowEmptySelection
                    itemComponent={props => (
                        <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                            <Select.ItemLabel>{props.item.rawValue.name}</Select.ItemLabel>
                            <Select.ItemIndicator class="text-blue-600">✓</Select.ItemIndicator>
                        </Select.Item>
                    )}
                    class="flex-grow md:flex-grow-0"
                >
                    <Select.Label class="sr-only">API Key</Select.Label>
                    <Select.Trigger class="form-select flex justify-between items-center w-full px-3 py-2 text-base font-normal text-gray-700 bg-white bg-clip-padding bg-no-repeat border border-solid border-gray-300 rounded transition ease-in-out m-0 focus:text-gray-700 focus:bg-white focus:border-blue-600 focus:outline-none shadow-sm" aria-label="Filter by API Key">
                        <Select.Value<GlobalApiKeyItem>>
                            {state => state.selectedOption()!.name}
                        </Select.Value>
                        <Select.Icon class="ml-2 text-gray-500">▼</Select.Icon>
                    </Select.Trigger>
                    <Select.Portal>
                        <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                            <Select.Listbox class="max-h-60 overflow-y-auto py-1">
                            </Select.Listbox>
                        </Select.Content>
                    </Select.Portal>
                </Select>

                {/* Provider Filter - Use Select instead of Select.Root */}
                <Select<ProviderListItem>
                    value={providersForSelect().find(p => p.provider.id === filters().provider_id)}
                    options={providersForSelect()}
                    optionValue={item => item.provider.id} // Use accessor for complex object
                    optionTextValue={item => item.provider.name} // Use accessor for complex object
                    onChange={(selectedItem) => setFilters(f => ({ ...f, provider_id: selectedItem!.provider.id }))}
                    disallowEmptySelection
                    itemComponent={props => (
                        <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                            {/* Access provider.name for display in the dropdown list */}
                            <Select.ItemLabel>{props.item.rawValue.provider.name}</Select.ItemLabel>
                            <Select.ItemIndicator class="text-blue-600">✓</Select.ItemIndicator>
                        </Select.Item>
                    )}
                    class="flex-grow md:flex-grow-0"
                >
                    <Select.Label class="sr-only">Provider</Select.Label>
                    <Select.Trigger class="form-select flex justify-between items-center w-full px-3 py-2 text-base font-normal text-gray-700 bg-white bg-clip-padding bg-no-repeat border border-solid border-gray-300 rounded transition ease-in-out m-0 focus:text-gray-700 focus:bg-white focus:border-blue-600 focus:outline-none shadow-sm" aria-label="Filter by Provider">
                        {/* Access provider.name for display in the trigger */}
                        <Select.Value<ProviderListItem>>
                            {state => state.selectedOption()!.provider.name}
                        </Select.Value>
                        <Select.Icon class="ml-2 text-gray-500">▼</Select.Icon>
                    </Select.Trigger>
                    <Select.Portal>
                        <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                            <Select.Listbox class="max-h-60 overflow-y-auto py-1">
                            </Select.Listbox>
                        </Select.Content>
                    </Select.Portal>
                </Select>

                {/* Model Name Filter */}
                <TextField
                    value={modelNameInput()} // Bind to the intermediate signal
                    onChange={setModelNameInput} // Update the intermediate signal directly
                    class="flex-grow md:flex-grow-0"
                >
                    <TextField.Label class="sr-only">Model Name</TextField.Label>
                    <TextField.Input
                        placeholder="Model Name"
                        class="form-input block w-full px-3 py-2 text-base font-normal text-gray-700 bg-white bg-clip-padding border border-solid border-gray-300 rounded transition ease-in-out m-0 focus:text-gray-700 focus:bg-white focus:border-blue-600 focus:outline-none shadow-sm"
                        aria-label="Filter by Model Name"
                    />
                </TextField>

                {/* Action Buttons */}
                <div class="flex gap-2 flex-wrap">
                    <Button
                        onClick={applyFilter}
                        class="btn bg-blue-600 hover:bg-blue-700 text-white font-medium py-2 px-4 rounded shadow-sm transition duration-150 ease-in-out"
                    >
                        {t('recordPage.filter.applyButton')}
                    </Button>
                    <Show when={filters().api_key_id || filters().provider_id || filters().model_name}>
                        <Button
                            onClick={resetFilter}
                            class="btn bg-gray-500 hover:bg-gray-600 text-white font-medium py-2 px-4 rounded shadow-sm transition duration-150 ease-in-out"
                        >
                            {t('recordPage.filter.resetButton')}
                        </Button>
                    </Show>
                </div>
            </div>

            {/* Data Table */}
            <Show when={recordsResult.loading}>
                <div class="text-center py-4 text-gray-500">{t('recordPage.loading')}</div>
            </Show>
            <Show when={!recordsResult.loading && recordsResult.error}>
                <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                    {t('recordPage.errorPrefix')} {recordsResult.error instanceof Error ? recordsResult.error.message : t('recordPage.unknownError')}
                </div>
            </Show>
            <Show when={!recordsResult.loading && !recordsResult.error}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <table class="min-w-full divide-y divide-gray-200">
                        <thead class="bg-gray-100">
                            <tr>
                                {/* Updated headers */}
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.modelName')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.provider')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.apiKey')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.status')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.promptTokens')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.completionTokens')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.reasoningTokens')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.totalTokens')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.stream')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.firstResp')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.totalResp')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.tps')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.cost')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.requestTime')}</th>
                                <th scope="col" class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('recordPage.table.details')}</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">
                            <For each={recordsResult()?.list} fallback={
                                <tr>
                                    <td colSpan={15} class="text-center py-6 text-gray-500"> {/* Updated colSpan */}
                                        <Show when={recordsResult()?.total === 0}>{t('recordPage.table.noRecordsMatch')}</Show>
                                        <Show when={!recordsResult()?.total}>{t('recordPage.table.noRecordsAvailable')}</Show>
                                    </td>
                                </tr>
                            }>
                                {(record: RecordItem) => (
                                    <tr class="hover:bg-gray-50 transition duration-150 ease-in-out">
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800">{record.model_name || '/'}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{record.providerName}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{record.apiKeyName}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm">
                                            <span class={`px-2 inline-flex text-xs leading-5 font-semibold rounded-full ${record.status === 'SUCCESS' ? 'bg-green-100 text-green-800' :
                                                    record.status === 'ERROR' ? 'bg-red-100 text-red-800' :
                                                        record.status === 'PENDING' ? 'bg-yellow-100 text-yellow-800' :
                                                            'bg-gray-100 text-gray-800'
                                                }`}>
                                                {record.status || t('common.notAvailable')}
                                            </span>
                                        </td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.prompt_tokens ?? '/'}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.completion_tokens ?? '/'}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.reasoning_tokens ?? '/'}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.total_tokens ?? '/'}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{record.isStreamDisplay}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.firstRespTimeDisplay}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.totalRespTimeDisplay}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.tpsDisplay}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600 text-right">{record.costDisplay}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{record.request_at_formatted}</td>
                                        <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">
                                            <Popover
                                                gutter={8}
                                                onOpenChange={(isOpen) => isOpen ? setExpandedRecordId(record.id) : setExpandedRecordId(null)}
                                            >
                                                <Popover.Trigger asChild>
                                                    <button class="text-blue-600 hover:text-blue-800 text-sm font-medium focus:outline-none">
                                                        {t('recordPage.table.viewDetails')}
                                                    </button>
                                                </Popover.Trigger>
                                                <Popover.Portal>
                                                    <Popover.Content class="bg-white border border-gray-300 rounded-md shadow-lg p-4 z-50 max-w-lg max-h-96 overflow-auto">
                                                        <Show
                                                            when={!detailedRecordData.loading && detailedRecordData()}
                                                            fallback={<p>{detailedRecordData.loading ? t('recordPage.loading') : t('recordPage.errorPrefix')}</p>}
                                                        >
                                                            <pre class="text-xs bg-gray-50 p-2 rounded-md whitespace-pre-wrap break-all">
                                                                {JSON.stringify(detailedRecordData(), null, 2)}
                                                            </pre>
                                                        </Show>
                                                    </Popover.Content>
                                                </Popover.Portal>
                                            </Popover>
                                        </td>
                                    </tr>
                                )}
                            </For>
                        </tbody>
                    </table>
                </div>
            </Show>

            {/* Pagination Controls */}
            <Show when={totalPages() > 0}>
                <div class="flex items-center justify-between mt-4 flex-wrap gap-4 px-4 py-3 bg-white border border-gray-200 rounded-lg shadow-sm">
                    {/* Kobalte Pagination */}
                    <Pagination
                        count={totalPages()}
                        page={currentPage()}
                        onPageChange={setCurrentPage}
                        itemComponent={props => (
                            <Pagination.Item
                                page={props.page}
                                class={`px-3 py-1.5 rounded border text-sm font-medium transition-colors duration-150 ease-in-out ${props.page === currentPage()
                                    ? 'bg-blue-600 text-white border-blue-600 z-10'
                                    : 'bg-white text-gray-700 border-gray-300 hover:bg-gray-50 ui-disabled:opacity-50 ui-disabled:cursor-not-allowed'
                                    }`}
                            >
                                {props.page}
                            </Pagination.Item>
                        )}
                        ellipsisComponent={() => (
                            <Pagination.Ellipsis class="px-3 py-1.5 text-gray-500">...</Pagination.Ellipsis>
                        )}
                        class={styles.pagination}
                    >
                        <Pagination.Previous class="px-3 py-1.5 rounded border border-gray-300 bg-white text-gray-600 hover:bg-gray-100 ui-disabled:opacity-50 ui-disabled:cursor-not-allowed transition duration-150 ease-in-out" aria-label={t('recordPage.pagination.previousPage')}>
                            {'<'}
                        </Pagination.Previous>
                        <Pagination.Items />
                        <Pagination.Next class="px-3 py-1.5 rounded border border-gray-300 bg-white text-gray-600 hover:bg-gray-100 ui-disabled:opacity-50 ui-disabled:cursor-not-allowed transition duration-150 ease-in-out" aria-label={t('recordPage.pagination.nextPage')}>
                            {'>'}
                        </Pagination.Next>
                    </Pagination>

                    {/* Page Info and Size Selector */}
                    <div class="flex items-center gap-4 flex-wrap">
                        <div class="text-sm text-gray-700">
                            {t('recordPage.pagination.page')} <span class="font-medium">{currentPage()}</span> {t('recordPage.pagination.of')} <span class="font-medium">{totalPages()}</span> (<span class="font-medium">{recordsResult()?.total ?? 0}</span> {t('recordPage.pagination.items')})
                        </div>
                        <div class="flex items-center space-x-2">
                            <label for="page-size-select" class="text-sm text-gray-700 whitespace-nowrap">{t('recordPage.pagination.itemsPerPage')}</label>
                            <Select<number>
                                value={pageSize()}
                                options={[10, 25, 50, 100]}
                                onChange={(value) => {
                                    if (value) {
                                        localStorage.setItem('pageSize', String(value));
                                        setPageSize(value);
                                        setCurrentPage(1);
                                    }
                                }}
                                itemComponent={props => (
                                    <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                                        <Select.ItemLabel>{props.item.rawValue}</Select.ItemLabel>
                                        <Select.ItemIndicator class="text-blue-600">✓</Select.ItemIndicator>
                                    </Select.Item>
                                )}
                            >
                                <Select.Trigger class="form-select flex justify-between items-center w-auto px-3 py-1.5 text-base font-normal text-gray-700 bg-white bg-clip-padding bg-no-repeat border border-solid border-gray-300 rounded transition ease-in-out m-0 focus:text-gray-700 focus:bg-white focus:border-blue-600 focus:outline-none shadow-sm" aria-label="Select number of items per page">
                                    <Select.Value<number>>{state => state.selectedOption()}</Select.Value>
                                    <Select.Icon class="ml-2 text-gray-500">▼</Select.Icon>
                                </Select.Trigger>
                                <Select.Portal>
                                    <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                                        <Select.Listbox class="py-1">
                                        </Select.Listbox>
                                    </Select.Content>
                                </Select.Portal>
                            </Select>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    );
}
