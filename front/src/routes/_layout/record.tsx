import { createSignal, createEffect, For, Show, createResource, createMemo, Suspense, Switch, Match } from 'solid-js';
import type { Resource, Component } from 'solid-js';
import { createFileRoute } from '@tanstack/solid-router';
import { useI18n } from '@/i18n'; // Import the i18n hook
import { Button } from '@/components/ui/Button';
import {
    DialogRoot,
    DialogContent,
    DialogHeader,
    DialogFooter,
    DialogTitle,
} from '@/components/ui/Dialog';
import { Pagination } from '@/components/ui/Pagination';
import { Select } from '@/components/ui/Select';
import { TextField } from '@/components/ui/Input';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '@/components/ui/Table';
import { request } from '@/services/api'; // Import the centralized request function
import styles from './record.module.css';
import { parseSse } from '@/utils/sse';

import { providers as globalProviders, loadProviders } from '@/store/providerStore'; // Import global providers
import { apiKeys as globalApiKeys, loadApiKeys } from '@/store/apiKeyStore'; // Import global API keys
import type { ProviderListItem, ProviderBase, ApiKeyItem as GlobalApiKeyItem } from '@/store/types'; // Import shared types, rename ApiKeyItem to avoid conflict

interface SseEvent {
	id?: string;
	event: string;
	data?: string;
	retry?: string;
}

// --- Type Definitions ---
interface RecordItem {
    id: number;
    system_api_key_id: number | null;
    provider_id: number | null;
    model_id: number | null;
    provider_api_key_id: number | null;
    model_name: string | null;
    real_model_name: string | null;
    request_received_at: number;
    llm_request_sent_at: number | null;
    llm_response_first_chunk_at: number | null;
    llm_response_completed_at: number | null;
    response_sent_to_client_at: number | null;
    status: string | null;
    is_stream: boolean;
    calculated_cost: number | null;
    cost_currency: string | null;
    prompt_tokens: number | null;
    completion_tokens: number | null;
    reasoning_tokens: number | null;
    total_tokens: number | null;
    channel: string | null;
    external_id: string | null;
    created_at: number;
    updated_at: number;
    storage_type: string | null;
    user_request_body: string | null;
    llm_request_body: string | null;
    llm_response_body: string | null;
    user_response_body: string | null;

    // Display-ready fields
    request_at_formatted?: string;
    providerName: string;
    apiKeyName: string;
    isStreamDisplay: string;
    firstRespTimeDisplay: string;
    totalRespTimeDisplay: string;
    tpsDisplay: string;
    costDisplay: string;
}

interface RecordDetail extends RecordItem {
    [key: string]: any; 
}

interface Filters {
    api_key_id: number;
    provider_id: number;
    status: string;
    search: string;
}

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

export const Route = createFileRoute('/_layout/record')({
    component: RecordPage,
});

function RecordPage() {
    const [t] = useI18n(); 

    loadApiKeys();
    loadProviders();

    const [currentPage, setCurrentPage] = createSignal(1);
    const [expandedRecordId, setExpandedRecordId] = createSignal<number | null>(null);
    const [isDetailModalOpen, setIsDetailModalOpen] = createSignal(false);

    const initialPageSize = () => {
        const storedSize = localStorage.getItem('pageSize');
        const size = storedSize ? parseInt(storedSize, 10) : 10;
        return !isNaN(size) && size > 0 ? size : 10; 
    };
    const [pageSize, setPageSize] = createSignal(initialPageSize());

    const [searchInput, setSearchInput] = createSignal('');
    let searchDebounceTimer: number;

    const [filters, setFilters] = createSignal<Filters>({
        api_key_id: 0,
        provider_id: 0,
        status: 'ALL',
        search: '',
    });

    const recordParams = () => ({
        page: currentPage(),
        size: pageSize(),
        currentFilters: filters()
    });

    const fetchRecordDetailsById = async (recordId: number | null): Promise<RecordDetail | null> => {
        if (recordId === null) {
            return null;
        }
        try {
            const response = await request(`/ai/manager/api/request_log/${recordId}`);
            return response;
        } catch (error) {
            console.error(`Failed to fetch details for record ${recordId}:`, error);
            throw error;
        }
    };
    const [detailedRecordData] = createResource(expandedRecordId, fetchRecordDetailsById);

    const totalPages = () => {
        const totalItems = recordsResult()?.total ?? 0;
        const size = pageSize();
        return size > 0 ? Math.ceil(totalItems / size) : 0;
    };

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

    const apiKeyOptions = createMemo(() => {
        const allKey = { value: 0, label: t('recordPage.filter.allApiKeys') };
        const keys = (globalApiKeys() || []).map(k => ({ value: k.id, label: k.name }));
        return [allKey, ...keys];
    });

    const providerOptions = createMemo(() => {
        const allProvider = { value: 0, label: t('recordPage.filter.allProviders') };
        const providers = (globalProviders() || []).map(p => ({ value: p.provider.id, label: p.provider.name }));
        return [allProvider, ...providers];
    });

    const statusOptions = createMemo(() => {
        const allStatus = { value: 'ALL', label: t('recordPage.filter.allStatuses') };
        const statuses = ['SUCCESS', 'PENDING', 'ERROR'].map(s => ({ value: s, label: t(`recordPage.filter.status.${s}`) }));
        return [allStatus, ...statuses];
    });

    const fetchRecords = async ({ page, size, currentFilters }: FetchRecordsParams): Promise<FetchRecordsResult> => {
        try {
            let queryParams = `page=${page}&page_size=${size}`;
            if (currentFilters.provider_id) {
                queryParams += `&provider_id=${encodeURIComponent(currentFilters.provider_id)}`;
            }
            if (currentFilters.search) {
                queryParams += `&search=${encodeURIComponent(currentFilters.search)}`;
            }
            if (currentFilters.status && currentFilters.status !== 'ALL') {
                queryParams += `&status=${encodeURIComponent(currentFilters.status)}`;
            }
            if (currentFilters.api_key_id) {
                queryParams += `&system_api_key_id=${encodeURIComponent(currentFilters.api_key_id)}`;
            }
            const result: any = await request(`/ai/manager/api/request_log/list?${queryParams}`);

            const list: RecordItem[] = (result?.list || []).map((backendRecord: any) => {
                let request_at_formatted = '/';
                if (backendRecord.request_received_at) {
                    try {
                        const date = new Date(backendRecord.request_received_at);
                        if (!isNaN(date.getTime())) {
                            request_at_formatted = new Intl.DateTimeFormat('sv-SE', {
                                year: 'numeric', month: '2-digit', day: '2-digit',
                                hour: '2-digit', minute: '2-digit', second: '2-digit'
                            }).format(date);
                        }
                    } catch (e) {
                        console.error("Error formatting date:", e);
                    }
                }
                const status = backendRecord.status || null;

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
                    request_received_at: backendRecord.request_received_at,
                    llm_request_sent_at: backendRecord.llm_request_sent_at ?? null,
                    llm_response_first_chunk_at: backendRecord.llm_response_first_chunk_at ?? null,
                    llm_response_completed_at: backendRecord.llm_response_completed_at ?? null,
                    calculated_cost: backendRecord.calculated_cost ?? null,
                    cost_currency: backendRecord.cost_currency || null,
                    channel: backendRecord.channel || null,
                    external_id: backendRecord.external_id || null,
                    model_id: backendRecord.model_id ?? null,
                    provider_api_key_id: backendRecord.provider_api_key_id ?? null,
                    real_model_name: backendRecord.real_model_name || null,
                    response_sent_to_client_at: backendRecord.response_sent_to_client_at ?? null,
                    created_at: backendRecord.created_at,
                    updated_at: backendRecord.updated_at,
                    storage_type: backendRecord.storage_type || null,
                    user_request_body: backendRecord.user_request_body || null,
                    llm_request_body: backendRecord.llm_request_body || null,
                    llm_response_body: backendRecord.llm_response_body || null,
                    user_response_body: backendRecord.user_response_body || null,
                    request_at_formatted,
                    providerName,
                    apiKeyName,
                    isStreamDisplay,
                    firstRespTimeDisplay,
                    totalRespTimeDisplay,
                    tpsDisplay,
                    costDisplay,
                };
            });

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
        setSearchInput('');
        setFilters({ api_key_id: 0, provider_id: 0, status: 'ALL', search: '' });
        setCurrentPage(1);
    };

    createEffect(() => {
        const currentSearch = searchInput();
        clearTimeout(searchDebounceTimer);
        searchDebounceTimer = window.setTimeout(() => {
            setFilters(f => ({ ...f, search: currentSearch }));
        }, 600);
    });

    const formatDate = (timestamp: number | null | undefined) => {
        if (!timestamp) return '/';
        try {
            const date = new Date(timestamp);
            if (isNaN(date.getTime())) return '/';
            return date.toISOString().replace('T', ' ').substring(0, 23);
        } catch (e) {
            console.error("Error formatting date:", e);
            return '/';
        }
    };

    const StatusBadge: Component<{ status: string | null }> = (props) => {
        const statusClass = () => {
            switch (props.status) {
                case 'SUCCESS': return 'bg-green-100 text-green-800';
                case 'ERROR': return 'bg-red-100 text-red-800';
                case 'PENDING': return 'bg-yellow-100 text-yellow-800';
                default: return 'bg-gray-100 text-gray-800';
            }
        };
        return (
            <span class={`px-2 inline-flex text-xs leading-5 font-semibold rounded-full ${statusClass()}`}>
                {props.status || t('common.notAvailable')}
            </span>
        );
    };

    const DetailItem: Component<{ label: string; children: any; }> = (props) => {
        return (
            <div class="py-2 sm:grid sm:grid-cols-3 sm:gap-4">
                <dt class="text-sm font-medium text-gray-500">{props.label}</dt>
                <dd class="mt-1 text-sm text-gray-900 sm:mt-0 sm:col-span-2">{props.children ?? '/'}</dd>
            </div>
        );
    };

    const JsonViewer: Component<{ data: any; title: string }> = (props) => {
        const contentToDisplay = createMemo(() => {
            if (props.data === null || props.data === undefined) return null;
            if (typeof props.data === 'string') {
                try {
                    return JSON.stringify(JSON.parse(props.data), null, 2);
                } catch (error) {
                    return props.data;
                }
            }
            try {
                return JSON.stringify(props.data, null, 2);
            } catch (error) {
                return 'Error: Could not display object.';
            }
        });

        return (
            <Show when={contentToDisplay()}>
                <div>
                    <h4 class="text-md font-medium text-gray-700">{props.title}</h4>
                    <div class="mt-1 text-xs bg-gray-50 p-2 rounded-md max-h-[40rem] overflow-y-auto">
                        <pre class="whitespace-pre-wrap break-all">{contentToDisplay()}</pre>
                    </div>
                </div>
            </Show>
        );
    };

    const SseEventViewer: Component<{ event: SseEvent }> = (props) => {
        const eventData = createMemo(() => {
            if (!props.event.data) return { type: 'empty' };
            try {
                return { type: 'json', content: JSON.stringify(JSON.parse(props.event.data), null, 2) };
            } catch (e) {
                return { type: 'text', content: props.event.data };
            }
        });
    
        return (
            <div class="mb-2 border-b border-gray-200 pb-2 last:border-b-0 last:pb-0">
                <p class="font-semibold text-gray-600">event: {props.event.event}</p>
                <Show when={eventData().type !== 'empty'}>
                    <pre class="mt-1 whitespace-pre-wrap break-all">{eventData().content}</pre>
                </Show>
            </div>
        );
    };

    const ResponseBodyViewer: Component<{ data: any; title: string; status: string | null; }> = (props) => {
        const contentToDisplay = createMemo(() => {
            if (props.data === null || props.data === undefined) return { type: 'empty' };
            const dataString = typeof props.data === 'string' ? props.data : JSON.stringify(props.data);
    
            try {
                return { type: 'json', content: JSON.stringify(JSON.parse(dataString), null, 2) };
            } catch (e) { /* Not JSON */ }
    
            if (props.status === 'SUCCESS') {
                try {
                    const sseEvents = parseSse(dataString);
                    if (sseEvents.length > 0) return { type: 'sse', content: sseEvents };
                } catch (e) { /* Not SSE */ }
            }
    
            return { type: 'text', content: dataString };
        });
    
        return (
            <Show when={contentToDisplay().type !== 'empty'}>
                <div>
                    <h4 class="text-md font-medium text-gray-700">{props.title}</h4>
                    <div class="mt-1 text-xs bg-gray-50 p-2 rounded-md max-h-[40rem] overflow-y-auto">
                        <Switch>
                            <Match when={contentToDisplay().type === 'json' || contentToDisplay().type === 'text'}>
                                <pre class="whitespace-pre-wrap break-all">{contentToDisplay().content}</pre>
                            </Match>
                            <Match when={contentToDisplay().type === 'sse'}>
                                <For each={contentToDisplay().content as SseEvent[]}>
                                    {(event) => <SseEventViewer event={event} />}
                                </For>
                            </Match>
                        </Switch>
                    </div>
                </div>
            </Show>
        );
    };
    
    const RemoteLogViewer: Component<{ path: string | null | undefined, title: string }> = (props) => {
        const [content, setContent] = createSignal<string | null>(null);
        const [isLoading, setIsLoading] = createSignal(false);
        const [error, setError] = createSignal<string | null>(null);
    
        const fetchContent = async () => {
            if (!props.path) return;
            setIsLoading(true);
            setError(null);
            try {
                const data = await request<any>(`/ai/manager/api${props.path}`);
                setContent(typeof data === 'string' ? data : JSON.stringify(data));
            } catch (err) {
                setError((err as Error).message || 'Failed to fetch log content.');
            } finally {
                setIsLoading(false);
            }
        };
    
        createEffect(() => {
            if (props.path) fetchContent();
        });
    
        return (
            <div>
                <Show when={isLoading()}><p>Loading...</p></Show>
                <Show when={error()}><p class="text-red-600">Error: {error()}</p></Show>
                <Show when={content()}>
                    <JsonViewer data={content()} title={props.title} />
                </Show>
                <Show when={!isLoading() && !content() && !error() && props.path}>
                    <p>No content to display.</p>
                </Show>
            </div>
        );
    };

    const RemoteResponseBodyViewer: Component<{ path: string | null | undefined, title: string, status: string | null }> = (props) => {
        const [content, setContent] = createSignal<string | null>(null);
        const [isLoading, setIsLoading] = createSignal(false);
        const [error, setError] = createSignal<string | null>(null);
    
        const fetchContent = async () => {
            if (!props.path) return;
            setIsLoading(true);
            setError(null);
            try {
                const data = await request<any>(`/ai/manager/api${props.path}`);
                setContent(typeof data === 'string' ? data : JSON.stringify(data));
            } catch (err) {
                setError((err as Error).message || 'Failed to fetch log content.');
            } finally {
                setIsLoading(false);
            }
        };
    
        createEffect(() => {
            if (props.path) fetchContent();
        });
    
        return (
            <div>
                <Show when={isLoading()}><p>Loading...</p></Show>
                <Show when={error()}><p class="text-red-600">Error: {error()}</p></Show>
                <Show when={content()}>
                    <ResponseBodyViewer data={content()} title={props.title} status={props.status} />
                </Show>
                <Show when={!isLoading() && !content() && !error() && props.path}>
                    <p>No content to display.</p>
                </Show>
            </div>
        );
    };

    return (
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-semibold mb-4 text-gray-800">{t('recordPage.title')}</h1>

            <div class="flex flex-wrap items-center gap-4 p-4 bg-gray-50 rounded-lg shadow-sm border border-gray-200">
                <Select
                    value={apiKeyOptions().find(k => k.value === filters().api_key_id)}
                    onChange={(selectedItem) => setFilters(f => ({ ...f, api_key_id: selectedItem!.value }))}
                    optionValue="value"
                    optionTextValue="label"
                    options={apiKeyOptions()}
                    class="flex-grow md:flex-grow-0"
                />
                <Select
                    value={providerOptions().find(p => p.value === filters().provider_id)}
                    onChange={(selectedItem) => setFilters(f => ({ ...f, provider_id: selectedItem!.value }))}
                    optionValue="value"
                    optionTextValue="label"
                    options={providerOptions()}
                    class="flex-grow md:flex-grow-0"
                />
                <Select
                    value={statusOptions().find(s => s.value === filters().status)}
                    onChange={(selectedItem) => setFilters(f => ({ ...f, status: selectedItem!.value }))}
                    optionValue="value"
                    optionTextValue="label"
                    options={statusOptions()}
                    class="flex-grow md:flex-grow-0"
                />
                <TextField
                    value={searchInput()}
                    onChange={setSearchInput}
                    placeholder={t('recordPage.filter.searchPlaceholder')}
                    class="flex-grow md:flex-grow-0"
                />
                <div class="flex gap-2 flex-wrap">
                    <Button onClick={applyFilter} variant="primary">
                        {t('recordPage.filter.applyButton')}
                    </Button>
                    <Show when={filters().api_key_id || filters().provider_id || filters().status !== 'ALL' || filters().search}>
                        <Button onClick={resetFilter} variant="secondary">
                            {t('recordPage.filter.resetButton')}
                        </Button>
                    </Show>
                </div>
            </div>

            <Suspense fallback={<div class="text-center py-4 text-gray-500">{t('recordPage.loading')}</div>}>
                <Show
                    when={recordsResult.error}
                    fallback={
                        <div classList={{ 'opacity-50 pointer-events-none': recordsResult.state === 'refreshing' }}>
                            <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                                <TableRoot size="small">
                                    <TableHeader>
                                        <TableRow>
                                            <TableColumnHeader>{t('recordPage.table.modelName')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.provider')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.apiKey')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.channel')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.externalId')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.status')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.promptTokens')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.completionTokens')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.reasoningTokens')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.totalTokens')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.stream')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.firstResp')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.totalResp')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.tps')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.cost')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.requestTime')}</TableColumnHeader>
                                            <TableColumnHeader>{t('recordPage.table.details')}</TableColumnHeader>
                                        </TableRow>
                                    </TableHeader>
                                    <TableBody>
                                        <For each={recordsResult()?.list} fallback={
                                            <TableRow>
                                                <TableCell colSpan={17} class="text-center py-6">
                                                    <Show when={recordsResult()?.total === 0}>{t('recordPage.table.noRecordsMatch')}</Show>
                                                    <Show when={!recordsResult()?.total}>{t('recordPage.table.noRecordsAvailable')}</Show>
                                                </TableCell>
                                            </TableRow>
                                        }>
                                            {(record: RecordItem) => (
                                                <TableRow>
                                                    <TableCell>{record.model_name || '/'}</TableCell>
                                                    <TableCell>{record.providerName}</TableCell>
                                                    <TableCell>{record.apiKeyName}</TableCell>
                                                    <TableCell>{record.channel || '/'}</TableCell>
                                                    <TableCell>{record.external_id || '/'}</TableCell>
                                                    <TableCell><StatusBadge status={record.status} /></TableCell>
                                                    <TableCell class="text-right">{record.prompt_tokens ?? '/'}</TableCell>
                                                    <TableCell class="text-right">{record.completion_tokens ?? '/'}</TableCell>
                                                    <TableCell class="text-right">{record.reasoning_tokens ?? '/'}</TableCell>
                                                    <TableCell class="text-right">{record.total_tokens ?? '/'}</TableCell>
                                                    <TableCell>{record.isStreamDisplay}</TableCell>
                                                    <TableCell class="text-right">{record.firstRespTimeDisplay}</TableCell>
                                                    <TableCell class="text-right">{record.totalRespTimeDisplay}</TableCell>
                                                    <TableCell class="text-right">{record.tpsDisplay}</TableCell>
                                                    <TableCell class="text-right">{record.costDisplay}</TableCell>
                                                    <TableCell>{record.request_at_formatted}</TableCell>
                                                    <TableCell>
                                                        <button
                                                            class="text-blue-600 hover:text-blue-800 text-sm font-medium focus:outline-none"
                                                            onClick={() => {
                                                                setExpandedRecordId(record.id);
                                                                setIsDetailModalOpen(true);
                                                            }}
                                                        >
                                                            {t('recordPage.table.viewDetails')}
                                                        </button>
                                                    </TableCell>
                                                </TableRow>
                                            )}
                                        </For>
                                    </TableBody>
                                </TableRoot>
                            </div>

                            <Show when={totalPages() > 0}>
                                <div class="flex items-center justify-between mt-4 flex-wrap gap-4 px-4 py-3 bg-white border border-gray-200 rounded-lg shadow-sm">
                                    <Pagination
                                        count={totalPages()}
                                        page={currentPage()}
                                        onPageChange={setCurrentPage}
                                        itemComponent={props => <Pagination.Item page={props.page}>{props.page}</Pagination.Item>}
                                        ellipsisComponent={() => <Pagination.Ellipsis />}
                                        class={styles.pagination}
                                    >
                                        <Pagination.Previous aria-label={t('recordPage.pagination.previousPage')} />
                                        <Pagination.Items />
                                        <Pagination.Next aria-label={t('recordPage.pagination.nextPage')} />
                                    </Pagination>
                                    <div class="flex items-center gap-4 flex-wrap">
                                        <div class="text-sm text-gray-700">
                                            {t('recordPage.pagination.page')} <span class="font-medium">{currentPage()}</span> {t('recordPage.pagination.of')} <span class="font-medium">{totalPages()}</span> (<span class="font-medium">{recordsResult()?.total ?? 0}</span> {t('recordPage.pagination.items')})
                                        </div>
                                        <div class="flex items-center space-x-2">
                                            <label for="page-size-select" class="text-sm text-gray-700 whitespace-nowrap">{t('recordPage.pagination.itemsPerPage')}</label>
                                            <Select
                                                value={pageSize()}
                                                options={[10, 25, 50, 100]}
                                                onChange={(value) => {
                                                    if (value) {
                                                        localStorage.setItem('pageSize', String(value));
                                                        setPageSize(value);
                                                        setCurrentPage(1);
                                                    }
                                                }}
                                                class="w-auto"
                                            />
                                        </div>
                                    </div>
                                </div>
                            </Show>
                        </div>
                    }
                >
                    <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                        {t('recordPage.errorPrefix')} {recordsResult.error instanceof Error ? recordsResult.error.message : t('recordPage.unknownError')}
                    </div>
                </Show>
            </Suspense>
            <DialogRoot open={isDetailModalOpen()} onOpenChange={setIsDetailModalOpen}>
                <DialogContent class="max-w-7xl">
                    <DialogHeader>
                        <DialogTitle>{t('recordPage.detailModal.title', { defaultValue: 'Log Details' })}</DialogTitle>
                    </DialogHeader>
                    <div class="py-4 max-h-[70vh] overflow-y-auto">
                        <Show when={!detailedRecordData.loading && detailedRecordData()} fallback={<p>{t('recordPage.loading')}</p>}>
                            {(record) => (
                                <div class="space-y-6 text-sm">
                                    <section>
                                        <h3 class="text-base font-semibold text-gray-900 border-b pb-2 mb-2">General</h3>
                                        <dl class="divide-y divide-gray-100">
                                            <DetailItem label="ID">{record().id}</DetailItem>
                                            <DetailItem label="Status"><StatusBadge status={record().status} /></DetailItem>
                                            <DetailItem label="Provider">{record().provider_id != null ? providerMap().get(record().provider_id) || '/' : '/'}</DetailItem>
                                            <DetailItem label="API Key">{record().system_api_key_id != null ? apiKeyMap().get(record().system_api_key_id) || '/' : '/'}</DetailItem>
                                            <DetailItem label="Model Name">{record().model_name}</DetailItem>
                                            <DetailItem label="Real Model Name">{record().real_model_name}</DetailItem>
                                            <DetailItem label="Channel">{record().channel}</DetailItem>
                                            <DetailItem label="External ID">{record().external_id}</DetailItem>
                                            <DetailItem label="Stream">{record().is_stream ? t('common.yes') : t('common.no')}</DetailItem>
                                        </dl>
                                    </section>
                                    <section>
                                        <h3 class="text-base font-semibold text-gray-900 border-b pb-2 mb-2">Timings (UTC)</h3>
                                        <dl class="divide-y divide-gray-100">
                                            <DetailItem label="Request Received">{formatDate(record().request_received_at)}</DetailItem>
                                            <DetailItem label="LLM Request Sent">{formatDate(record().llm_request_sent_at)}</DetailItem>
                                            <DetailItem label="LLM First Chunk">{formatDate(record().llm_response_first_chunk_at)}</DetailItem>
                                            <DetailItem label="LLM Completed">{formatDate(record().llm_response_completed_at)}</DetailItem>
                                            <DetailItem label="Response to Client">{formatDate(record().response_sent_to_client_at)}</DetailItem>
                                        </dl>
                                    </section>
                                    <section>
                                        <h3 class="text-base font-semibold text-gray-900 border-b pb-2 mb-2">Usage & Cost</h3>
                                        <dl class="divide-y divide-gray-100">
                                            <DetailItem label="Prompt Tokens">{record().prompt_tokens}</DetailItem>
                                            <DetailItem label="Completion Tokens">{record().completion_tokens}</DetailItem>
                                            <DetailItem label="Reasoning Tokens">{record().reasoning_tokens}</DetailItem>
                                            <DetailItem label="Total Tokens">{record().total_tokens}</DetailItem>
                                            <DetailItem label="Calculated Cost">{record().calculated_cost != null ? `${record().cost_currency || ''} ${(record().calculated_cost / 1000000000)}` : '/'}</DetailItem>
                                            <DetailItem label="Storage Type">{record().storage_type}</DetailItem>
                                        </dl>
                                    </section>
                                    <section>
                                        <h3 class="text-base font-semibold text-gray-900 border-b pb-2 mb-2">Payloads</h3>
                                        <div class="space-y-4">
                                            <Show
                                                when={record().user_request_body === record().llm_request_body}
                                                fallback={
                                                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                                        <div>
                                                            <Show 
                                                                when={record().storage_type}
                                                                fallback={<JsonViewer data={record().user_request_body} title="User Request Body" />}
                                                            >
                                                                <RemoteLogViewer path={record().user_request_body} title="User Request Body" />
                                                            </Show>
                                                        </div>
                                                        <div>
                                                            <Show 
                                                                when={record().storage_type}
                                                                fallback={<JsonViewer data={record().llm_request_body} title="LLM Request Body" />}
                                                            >
                                                                <RemoteLogViewer path={record().llm_request_body} title="LLM Request Body" />
                                                            </Show>
                                                        </div>
                                                    </div>
                                                }
                                            >
                                                <div>
                                                    <Show 
                                                        when={record().storage_type}
                                                        fallback={<JsonViewer data={record().user_request_body} title="User & LLM Request Body" />}
                                                    >
                                                        <RemoteLogViewer path={record().user_request_body} title="User & LLM Request Body" />
                                                    </Show>
                                                </div>
                                            </Show>

                                            <Show
                                                when={record().user_response_body === record().llm_response_body}
                                                fallback={
                                                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                                        <div>
                                                            <Show 
                                                                when={record().storage_type}
                                                                fallback={<ResponseBodyViewer data={record().llm_response_body} title="LLM Response Body" status={record().status} />}
                                                            >
                                                                <RemoteResponseBodyViewer path={record().llm_response_body} title="LLM Response Body" status={record().status} />
                                                            </Show>
                                                        </div>
                                                        <div>
                                                            <Show 
                                                                when={record().storage_type}
                                                                fallback={<ResponseBodyViewer data={record().user_response_body} title="User Response Body" status={record().status} />}
                                                            >
                                                                <RemoteResponseBodyViewer path={record().user_response_body} title="User Response Body" status={record().status} />
                                                            </Show>
                                                        </div>
                                                    </div>
                                                }
                                            >
                                                <div>
                                                    <Show 
                                                        when={record().storage_type}
                                                        fallback={<ResponseBodyViewer data={record().user_response_body} title="User & LLM Response Body" status={record().status} />}
                                                    >
                                                        <RemoteResponseBodyViewer path={record().user_response_body} title="User & LLM Response Body" status={record().status} />
                                                    </Show>
                                                </div>
                                            </Show>
                                        </div>
                                    </section>
                                </div>
                            )}
                        </Show>
                    </div>
                    <DialogFooter>
                        <Button variant="secondary" onClick={() => setIsDetailModalOpen(false)}>
                            {t('common.close', { defaultValue: 'Close' })}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>
        </div>
    );
}