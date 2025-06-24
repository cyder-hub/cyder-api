import { createSignal, For, Show, createResource, Accessor, Setter, onMount } from 'solid-js';
import type { Resource } from 'solid-js';
import { Button } from '@kobalte/core/button';
import { Select as KSelect } from '@kobalte/core/select'; // Aliased to avoid conflict if HTMLSelectElement is used
import { TextField } from '@kobalte/core/text-field';
import { request } from '../services/api';

// --- Type Definitions ---

interface Model {
    id: number;
    model_name: string;
}

interface Provider {
    id: number;
    name: string;
}

interface ProviderDetail {
    provider: Provider;
    models: Model[];
}

// For items in white/black lists in the UI state and form
interface ListStrategyItemUI {
    resource_type: 'provider' | 'model';
    provider_id: number | null;
    model_id: number | null;
}

// For items in quota lists in the UI state and form
interface QuotaStrategyItemUI {
    id?: number; // From backend, for existing items
    resource_type: 'global' | 'provider' | 'model';
    resource_id: number | null; // Provider ID or Model ID if type is provider/model
    limit_type: 'request' | 'fee';
    limit_value: number | null;
    duration: 'day' | 'hour' | 'minute';
}

interface LimitStrategyBase {
    name: string;
    main_strategy: 'default' | 'unlimited';
    description: string | null;
}

// Represents a full strategy in the UI, including items for editing
interface LimitStrategy extends LimitStrategyBase {
    id: number | null; // null for new strategy
    white_list: ListStrategyItemUI[];
    black_list: ListStrategyItemUI[];
    quota_list: QuotaStrategyItemUI[];
}

// What the API returns for a list of strategies (summary)
interface StrategySummaryFromAPI {
    id: number;
    name: string;
    main_strategy: 'default' | 'unlimited';
    description: string | null;
    white_list_count: number; // Assuming API might provide counts
    black_list_count: number;
    quota_list_count: number;
    // The old alpine code sums lengths: strategy.white_list.length + strategy.black_list.length + strategy.quota_list.length
    // If the list endpoint returns full items, we can sum them. If not, we need counts or fetch details.
    // For now, let's assume the list endpoint provides enough info or we adapt.
    // The alpine code fetches full list items: result.data.map(s => ({ ...s, items: s.items || [] }));
    // Let's assume the /ai/manager/api/limit_strategy/list returns items directly
    white_list: Array<{ resource_type: 'provider' | 'model'; resource_id: number | null }>;
    black_list: Array<{ resource_type: 'provider' | 'model'; resource_id: number | null }>;
    quota_list: Array<QuotaStrategyItemUI>; // Assuming quota items are returned as is
}


// What the API returns for a single strategy's details
interface StrategyDetailFromAPI extends LimitStrategyBase {
    id: number;
    white_list: Array<{ resource_type: 'provider' | 'model'; resource_id: number | null }>;
    black_list: Array<{ resource_type: 'provider' | 'model'; resource_id: number | null }>;
    quota_list: QuotaStrategyItemUI[];
}

const newStrategyTemplate = (): LimitStrategy => ({
    id: null,
    name: '',
    main_strategy: 'default',
    description: '',
    white_list: [],
    black_list: [],
    quota_list: []
});

const newWhiteListItemTemplate = (): ListStrategyItemUI => ({
    resource_type: 'provider',
    provider_id: null,
    model_id: null
});

const newBlackListItemTemplate = (): ListStrategyItemUI => ({
    resource_type: 'provider',
    provider_id: null,
    model_id: null
});

const newQuotaListItemTemplate = (): QuotaStrategyItemUI => ({
    resource_type: 'global',
    resource_id: null,
    limit_type: 'request',
    limit_value: null,
    duration: 'day'
});

// --- API Functions ---

const fetchStrategiesAPI = async (): Promise<StrategySummaryFromAPI[]> => {
    try {
        const response = await request("/ai/manager/api/limit_strategy/list");
        // Assuming response is { code: 0, data: [], message: '' }
        return response || [];
    } catch (error) {
        console.error("Failed to fetch strategies:", error);
        return [];
    }
};

const fetchProvidersWithModelsAPI = async (): Promise<ProviderDetail[]> => {
    try {
        const response = await request("/ai/manager/api/provider/detail/list");
        return response || [];
    } catch (error) {
        console.error("Failed to fetch providers with models:", error);
        return [];
    }
};

const fetchStrategyDetailAPI = async (id: number): Promise<StrategyDetailFromAPI | null> => {
    try {
        const response = await request(`/ai/manager/api/limit_strategy/${id}`);
        return response || null;
    } catch (error) {
        console.error("Failed to fetch strategy detail:", error);
        return null;
    }
};

const saveStrategyAPI = async (strategy: LimitStrategy): Promise<any> => {
    const payload = {
        name: strategy.name,
        main_strategy: strategy.main_strategy,
        description: strategy.description || null,
        white_list: strategy.white_list.map(item => ({
            resource_type: item.resource_type,
            resource_id: item.resource_type === 'provider' ? item.provider_id : item.model_id,
        })),
        black_list: strategy.black_list.map(item => ({
            resource_type: item.resource_type,
            resource_id: item.resource_type === 'provider' ? item.provider_id : item.model_id,
        })),
        quota_list: strategy.quota_list.map(item => ({
            ...item,
            limit_value: item.limit_value ?? 0, // Ensure limit_value is not null
        })),
    };

    const url = strategy.id ? `/ai/manager/api/limit_strategy/${strategy.id}` : '/ai/manager/api/limit_strategy';
    const method = strategy.id ? 'PUT' : 'POST';

    return request(url, {
        method: method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
    });
};

const deleteStrategyAPI = async (id: number): Promise<any> => {
    return request(`/ai/manager/api/limit_strategy/${id}`, { method: 'DELETE' });
};


// --- Helper to transform API detail to UI state ---
function transformApiDetailToUiState(apiDetail: StrategyDetailFromAPI, providersList: ProviderDetail[]): LimitStrategy {
    const findProviderForModel = (modelId: number | null) => {
        if (modelId === null) return null;
        const providerDetail = providersList.find(p => p.models.some(m => m.id === modelId));
        return providerDetail ? providerDetail.provider.id : null;
    };

    return {
        id: apiDetail.id,
        name: apiDetail.name,
        main_strategy: apiDetail.main_strategy,
        description: apiDetail.description,
        white_list: apiDetail.white_list.map(item => ({
            resource_type: item.resource_type,
            provider_id: item.resource_type === 'provider' ? item.resource_id : findProviderForModel(item.resource_id),
            model_id: item.resource_type === 'model' ? item.resource_id : null,
        })),
        black_list: apiDetail.black_list.map(item => ({
            resource_type: item.resource_type,
            provider_id: item.resource_type === 'provider' ? item.resource_id : findProviderForModel(item.resource_id),
            model_id: item.resource_type === 'model' ? item.resource_id : null,
        })),
        quota_list: apiDetail.quota_list.map(item => ({ ...item })), // Assuming direct map for quota
    };
}


export default function LimitStrategyPage() {
    const [strategies, { refetch: refetchStrategies }] = createResource<StrategySummaryFromAPI[]>(fetchStrategiesAPI, { initialValue: [] });
    const [providers] = createResource<ProviderDetail[]>(fetchProvidersWithModelsAPI, { initialValue: [] });

    const [showEditModal, setShowEditModal] = createSignal(false);
    const [editingStrategy, setEditingStrategy] = createSignal<LimitStrategy>(newStrategyTemplate());

    // Signals for the "add item" forms within the modal
    const [newWhiteItem, setNewWhiteItem] = createSignal<ListStrategyItemUI>(newWhiteListItemTemplate());
    const [newBlackItem, setNewBlackItem] = createSignal<ListStrategyItemUI>(newBlackListItemTemplate());
    // newQuotaItem is not a signal itself, but added directly to editingStrategy().quota_list

    const handleOpenAddModal = () => {
        setEditingStrategy(newStrategyTemplate());
        setNewWhiteItem(newWhiteListItemTemplate()); // Reset forms
        setNewBlackItem(newBlackListItemTemplate());
        setShowEditModal(true);
    };

    const handleOpenEditModal = async (id: number) => {
        const detail = await fetchStrategyDetailAPI(id);
        if (detail && providers()) {
            setEditingStrategy(transformApiDetailToUiState(detail, providers()));
            setNewWhiteItem(newWhiteListItemTemplate());
            setNewBlackItem(newBlackListItemTemplate());
            setShowEditModal(true);
        } else {
            alert("Failed to load strategy details.");
        }
    };

    const handleCloseModal = () => {
        setShowEditModal(false);
    };

    const handleSaveStrategy = async () => {
        if (!editingStrategy()) return;
        // Basic validation for quota_list items' limit_value
        for (const item of editingStrategy().quota_list) {
            if (item.limit_value === null || item.limit_value < 0) {
                alert("Quota limit value cannot be empty or negative.");
                return;
            }
        }
        try {
            await saveStrategyAPI(editingStrategy());
            setShowEditModal(false);
            refetchStrategies();
        } catch (error) {
            console.error("Failed to save strategy:", error);
            alert(`Error saving strategy: ${error instanceof Error ? error.message : 'Unknown error'}`);
        }
    };

    const handleDeleteStrategy = async (id: number) => {
        if (confirm("Are you sure you want to delete this strategy?")) {
            try {
                await deleteStrategyAPI(id);
                refetchStrategies();
            } catch (error) {
                console.error("Failed to delete strategy:", error);
                alert(`Error deleting strategy: ${error instanceof Error ? error.message : 'Unknown error'}`);
            }
        }
    };

    // --- List Item Management ---
    const addItemToList = (listType: 'white_list' | 'black_list' | 'quota_list') => {
        const currentStrategy = editingStrategy();
        if (!currentStrategy) return;

        let newItem: ListStrategyItemUI | QuotaStrategyItemUI;
        let resetFormFn: (() => void) | null = null;

        if (listType === 'white_list') {
            const formItem = newWhiteItem();
            if (formItem.resource_type === 'model' && formItem.model_id === null) {
                alert('Please select a model for the white list item.'); return;
            }
            if (formItem.resource_type === 'provider' && formItem.provider_id === null) {
                alert('Please select a provider for the white list item.'); return;
            }
            newItem = { ...formItem };
            resetFormFn = () => setNewWhiteItem(newWhiteListItemTemplate());
        } else if (listType === 'black_list') {
            const formItem = newBlackItem();
            if (formItem.resource_type === 'model' && formItem.model_id === null) {
                alert('Please select a model for the black list item.'); return;
            }
            if (formItem.resource_type === 'provider' && formItem.provider_id === null) {
                alert('Please select a provider for the black list item.'); return;
            }
            newItem = { ...formItem };
            resetFormFn = () => setNewBlackItem(newBlackListItemTemplate());
        } else { // quota_list
            newItem = newQuotaListItemTemplate(); // Add a blank quota item
        }

        setEditingStrategy({
            ...currentStrategy,
            [listType]: [...currentStrategy[listType], newItem]
        });

        if (resetFormFn) resetFormFn();
    };

    const removeItemFromList = (listType: 'white_list' | 'black_list' | 'quota_list', index: number) => {
        const currentStrategy = editingStrategy();
        if (!currentStrategy) return;
        const updatedList = [...currentStrategy[listType]];
        updatedList.splice(index, 1);
        setEditingStrategy({
            ...currentStrategy,
            [listType]: updatedList
        });
    };

    const updateListItemField = <T extends keyof ListStrategyItemUI>(listType: 'white_list' | 'black_list', index: number, field: T, value: ListStrategyItemUI[T]) => {
        const currentStrategy = editingStrategy();
        if (!currentStrategy) return;
        const list = currentStrategy[listType] as ListStrategyItemUI[];
        const updatedList = list.map((item, i) =>
            i === index ? { ...item, [field]: value } : item
        );
         // If resource_type changes, reset dependent fields
        if (field === 'resource_type') {
            const updatedItem = updatedList[index];
            updatedItem.provider_id = null;
            updatedItem.model_id = null;
        } else if (field === 'provider_id') {
             const updatedItem = updatedList[index];
            updatedItem.model_id = null; // Reset model if provider changes
        }
        setEditingStrategy({ ...currentStrategy, [listType]: updatedList });
    };
    
    const updateQuotaItemField = <T extends keyof QuotaStrategyItemUI>(index: number, field: T, value: QuotaStrategyItemUI[T]) => {
        const currentStrategy = editingStrategy();
        if (!currentStrategy) return;
        const updatedList = currentStrategy.quota_list.map((item, i) =>
            i === index ? { ...item, [field]: value } : item
        );
        setEditingStrategy({ ...currentStrategy, quota_list: updatedList });
    };


    // --- Helper functions for display ---
    const getProviderName = (providerId: number | null): string => {
        if (providerId === null) return '所有 Provider';
        const pDetail = providers()?.find(p => p.provider.id === providerId);
        return pDetail ? pDetail.provider.name : '未知 Provider';
    };

    const getModelName = (providerId: number | null, modelId: number | null): string => {
        if (modelId === null) return 'N/A';
        if (providerId === null) return '未知 Model (无 Provider)';
        const pDetail = providers()?.find(p => p.provider.id === providerId);
        if (!pDetail || !pDetail.models) return '未知 Model (Provider 无模型)';
        const model = pDetail.models.find(m => m.id === modelId);
        return model ? model.model_name : '未知 Model';
    };

    const getModelsForProvider = (providerId: number | null): Model[] => {
        if (providerId === null || !providers()) return [];
        const pDetail = providers()?.find(p => p.provider.id === providerId);
        return pDetail?.models || [];
    };


    return (
        <div class="p-4 space-y-6">
            <div class="flex justify-between items-center mb-4">
                <h1 class="text-2xl font-semibold text-gray-800">限制策略</h1>
                <Button onClick={handleOpenAddModal} class="btn btn-primary">添加策略</Button>
            </div>

            {/* Data Table */}
            <Show when={strategies.loading}>
                <div class="text-center py-4 text-gray-500">Loading strategies...</div>
            </Show>
            <Show when={!strategies.loading && strategies.error}>
                <div class="text-center py-4 text-red-500">Error loading strategies.</div>
            </Show>
            <Show when={!strategies.loading && !strategies.error && strategies()?.length === 0}>
                <div class="text-center py-4 text-gray-500">No strategies found.</div>
            </Show>
            <Show when={!strategies.loading && !strategies.error && strategies() && strategies()!.length > 0}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <table class="min-w-full divide-y divide-gray-200 data-table">
                        <thead class="bg-gray-100">
                            <tr>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">名称</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">主策略</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">描述</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">策略项</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">操作</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">
                            <For each={strategies()}>{(strategy) =>
                                <tr>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800">{strategy.name}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{strategy.main_strategy}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{strategy.description || '/'}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">
                                        {`${(strategy.white_list?.length || 0) + (strategy.black_list?.length || 0) + (strategy.quota_list?.length || 0)} 项`}
                                    </td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm space-x-2">
                                        <Button onClick={() => handleOpenEditModal(strategy.id)} class="btn btn-primary btn-sm">修改</Button>
                                        <Button onClick={() => handleDeleteStrategy(strategy.id)} class="btn btn-danger btn-sm">删除</Button>
                                    </td>
                                </tr>
                            }</For>
                        </tbody>
                    </table>
                </div>
            </Show>

            {/* Edit/Add Modal */}
            <Show when={showEditModal()}>
                <div class="fixed inset-0 bg-gray-500 bg-opacity-75 transition-opacity z-40" onClick={handleCloseModal}></div>
                <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
                    <div class="bg-white rounded-lg shadow-xl p-6 space-y-4 w-full max-w-3xl max-h-[90vh] overflow-y-auto model"> {/* Increased max-w, added max-h and overflow */}
                        <h2 class="text-xl font-semibold text-gray-800 model-title">{editingStrategy()?.id ? '编辑策略' : '添加策略'}</h2>

                        {/* Strategy Fields */}
                        <TextField class="form-item" value={editingStrategy()?.name || ''} onChange={(v) => setEditingStrategy(s => ({ ...s!, name: v }))}>
                            <TextField.Label class="form-label">名称</TextField.Label>
                            <TextField.Input class="form-input" />
                        </TextField>
                        <div class="form-item">
                            <KSelect<string>
                                value={editingStrategy()?.main_strategy || 'default'}
                                onChange={(v) => setEditingStrategy(s => ({ ...s!, main_strategy: v as 'default' | 'unlimited' }))}
                                options={['default', 'unlimited']}
                                placeholder="选择主策略"
                                itemComponent={props => (
                                    <KSelect.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-selected:font-semibold">
                                        <KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel>
                                        <KSelect.ItemIndicator>✓</KSelect.ItemIndicator>
                                    </KSelect.Item>
                                )}
                            >
                                <KSelect.Label class="form-label">主策略</KSelect.Label>
                                <KSelect.Trigger class="form-select w-full">
                                    <KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value>
                                </KSelect.Trigger>
                                <KSelect.Portal>
                                    <KSelect.Content class="bg-white border rounded shadow-lg">
                                        <KSelect.Listbox />
                                    </KSelect.Content>
                                </KSelect.Portal>
                            </KSelect>
                        </div>
                        <TextField class="form-item" value={editingStrategy()?.description || ''} onChange={(v) => setEditingStrategy(s => ({ ...s!, description: v }))}>
                            <TextField.Label class="form-label">描述</TextField.Label>
                            <TextField.Input as="textarea" rows={3} class="form-input" />
                        </TextField>

                        <hr class="my-6" />

                        {/* White List Section */}
                        <div class="strategy-list-section space-y-3">
                            <h4 class="text-lg font-medium">白名单 (White List)</h4>
                            <div class="max-h-60 overflow-y-auto border rounded p-2 mb-2 space-y-2">
                                <Show when={editingStrategy()?.white_list.length === 0}>
                                    <p class="text-sm text-gray-500 text-center py-2">无白名单项</p>
                                </Show>
                                <For each={editingStrategy()?.white_list}>{(item, index) =>
                                    <div class="p-2 border rounded bg-gray-50 space-y-2">
                                        <div class="flex gap-2 items-end">
                                            <div class="flex-1">
                                                <KSelect<ListStrategyItemUI['resource_type']>
                                                    value={item.resource_type}
                                                    onChange={(v) => updateListItemField('white_list', index(), 'resource_type', v)}
                                                    options={['provider', 'model']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">资源类型</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <div class="flex-1">
                                                <KSelect<number | null>
                                                    value={item.provider_id}
                                                    onChange={(v) => updateListItemField('white_list', index(), 'provider_id', v)}
                                                    options={providers()?.map(p => p.provider) || []}
                                                    optionValue="id" optionTextValue="name" placeholder="选择 Provider"
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">Provider</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Provider>>{state => state.selectedOption()?.name || '选择 Provider'}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <Show when={item.resource_type === 'model'}>
                                                <div class="flex-1">
                                                    <KSelect<number | null>
                                                        value={item.model_id}
                                                        onChange={(v) => updateListItemField('white_list', index(), 'model_id', v)}
                                                        options={getModelsForProvider(item.provider_id)}
                                                        optionValue="id" optionTextValue="model_name" placeholder="选择 Model"
                                                        disabled={!item.provider_id}
                                                        itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.model_name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                    >
                                                        <KSelect.Label class="form-label form-label-sm">Model</KSelect.Label>
                                                        <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Model>>{state => state.selectedOption()?.model_name || '选择 Model'}</KSelect.Value></KSelect.Trigger>
                                                        <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                    </KSelect>
                                                </div>
                                            </Show>
                                            <Button onClick={() => removeItemFromList('white_list', index())} class="btn btn-danger btn-sm self-end">删除</Button>
                                        </div>
                                    </div>
                                }</For>
                            </div>
                             {/* Add White List Item Form */}
                            <div class="add-item-form flex gap-2 items-end border-t pt-3">
                                <div class="flex-1">
                                    <KSelect<ListStrategyItemUI['resource_type']>
                                        value={newWhiteItem().resource_type}
                                        onChange={(v) => setNewWhiteItem(s => ({ ...s, resource_type: v, provider_id: null, model_id: null }))}
                                        options={['provider', 'model']}
                                        itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                    >
                                        <KSelect.Label class="form-label form-label-sm">资源类型</KSelect.Label>
                                        <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                        <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                    </KSelect>
                                </div>
                                <div class="flex-1">
                                     <KSelect<number | null>
                                        value={newWhiteItem().provider_id}
                                        onChange={(v) => setNewWhiteItem(s => ({ ...s, provider_id: v, model_id: null }))}
                                        options={providers()?.map(p => p.provider) || []}
                                        optionValue="id" optionTextValue="name" placeholder="选择 Provider"
                                        itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                    >
                                        <KSelect.Label class="form-label form-label-sm">Provider</KSelect.Label>
                                        <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Provider>>{state => state.selectedOption()?.name || '选择 Provider'}</KSelect.Value></KSelect.Trigger>
                                        <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                    </KSelect>
                                </div>
                                <Show when={newWhiteItem().resource_type === 'model'}>
                                    <div class="flex-1">
                                        <KSelect<number | null>
                                            value={newWhiteItem().model_id}
                                            onChange={(v) => setNewWhiteItem(s => ({ ...s, model_id: v }))}
                                            options={getModelsForProvider(newWhiteItem().provider_id)}
                                            optionValue="id" optionTextValue="model_name" placeholder="选择 Model"
                                            disabled={!newWhiteItem().provider_id}
                                            itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.model_name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                        >
                                            <KSelect.Label class="form-label form-label-sm">Model</KSelect.Label>
                                            <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Model>>{state => state.selectedOption()?.model_name || '选择 Model'}</KSelect.Value></KSelect.Trigger>
                                            <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                        </KSelect>
                                    </div>
                                </Show>
                                <Button onClick={() => addItemToList('white_list')} class="btn btn-secondary btn-sm self-end">添加白名单项</Button>
                            </div>
                        </div>

                        {/* Black List Section (similar to White List) */}
                        <div class="strategy-list-section space-y-3 mt-4">
                            <h4 class="text-lg font-medium">黑名单 (Black List)</h4>
                            <div class="max-h-60 overflow-y-auto border rounded p-2 mb-2 space-y-2">
                                <Show when={editingStrategy()?.black_list.length === 0}>
                                    <p class="text-sm text-gray-500 text-center py-2">无黑名单项</p>
                                </Show>
                                <For each={editingStrategy()?.black_list}>{(item, index) =>
                                    <div class="p-2 border rounded bg-gray-50 space-y-2">
                                        <div class="flex gap-2 items-end">
                                            <div class="flex-1">
                                                <KSelect<ListStrategyItemUI['resource_type']>
                                                    value={item.resource_type}
                                                    onChange={(v) => updateListItemField('black_list', index(), 'resource_type', v)}
                                                    options={['provider', 'model']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">资源类型</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <div class="flex-1">
                                                <KSelect<number | null>
                                                    value={item.provider_id}
                                                    onChange={(v) => updateListItemField('black_list', index(), 'provider_id', v)}
                                                    options={providers()?.map(p => p.provider) || []}
                                                    optionValue="id" optionTextValue="name" placeholder="选择 Provider"
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">Provider</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Provider>>{state => state.selectedOption()?.name || '选择 Provider'}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <Show when={item.resource_type === 'model'}>
                                                <div class="flex-1">
                                                    <KSelect<number | null>
                                                        value={item.model_id}
                                                        onChange={(v) => updateListItemField('black_list', index(), 'model_id', v)}
                                                        options={getModelsForProvider(item.provider_id)}
                                                        optionValue="id" optionTextValue="model_name" placeholder="选择 Model"
                                                        disabled={!item.provider_id}
                                                        itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.model_name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                    >
                                                        <KSelect.Label class="form-label form-label-sm">Model</KSelect.Label>
                                                        <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Model>>{state => state.selectedOption()?.model_name || '选择 Model'}</KSelect.Value></KSelect.Trigger>
                                                        <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                    </KSelect>
                                                </div>
                                            </Show>
                                            <Button onClick={() => removeItemFromList('black_list', index())} class="btn btn-danger btn-sm self-end">删除</Button>
                                        </div>
                                    </div>
                                }</For>
                            </div>
                            {/* Add Black List Item Form */}
                            <div class="add-item-form flex gap-2 items-end border-t pt-3">
                                <div class="flex-1">
                                    <KSelect<ListStrategyItemUI['resource_type']>
                                        value={newBlackItem().resource_type}
                                        onChange={(v) => setNewBlackItem(s => ({ ...s, resource_type: v, provider_id: null, model_id: null }))}
                                        options={['provider', 'model']}
                                        itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                    >
                                        <KSelect.Label class="form-label form-label-sm">资源类型</KSelect.Label>
                                        <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                        <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                    </KSelect>
                                </div>
                                <div class="flex-1">
                                     <KSelect<number | null>
                                        value={newBlackItem().provider_id}
                                        onChange={(v) => setNewBlackItem(s => ({ ...s, provider_id: v, model_id: null }))}
                                        options={providers()?.map(p => p.provider) || []}
                                        optionValue="id" optionTextValue="name" placeholder="选择 Provider"
                                        itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                    >
                                        <KSelect.Label class="form-label form-label-sm">Provider</KSelect.Label>
                                        <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Provider>>{state => state.selectedOption()?.name || '选择 Provider'}</KSelect.Value></KSelect.Trigger>
                                        <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                    </KSelect>
                                </div>
                                <Show when={newBlackItem().resource_type === 'model'}>
                                    <div class="flex-1">
                                        <KSelect<number | null>
                                            value={newBlackItem().model_id}
                                            onChange={(v) => setNewBlackItem(s => ({ ...s, model_id: v }))}
                                            options={getModelsForProvider(newBlackItem().provider_id)}
                                            optionValue="id" optionTextValue="model_name" placeholder="选择 Model"
                                            disabled={!newBlackItem().provider_id}
                                            itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue.model_name}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                        >
                                            <KSelect.Label class="form-label form-label-sm">Model</KSelect.Label>
                                            <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<Model>>{state => state.selectedOption()?.model_name || '选择 Model'}</KSelect.Value></KSelect.Trigger>
                                            <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                        </KSelect>
                                    </div>
                                </Show>
                                <Button onClick={() => addItemToList('black_list')} class="btn btn-secondary btn-sm self-end">添加黑名单项</Button>
                            </div>
                        </div>

                        {/* Quota List Section */}
                        <div class="strategy-list-section space-y-3 mt-4">
                            <div class="flex justify-between items-center">
                                <h4 class="text-lg font-medium">配额 (Quota List)</h4>
                                <Button onClick={() => addItemToList('quota_list')} class="btn btn-secondary btn-sm">添加配额项</Button>
                            </div>
                            <div class="max-h-60 overflow-y-auto border rounded p-2 space-y-2">
                                <Show when={editingStrategy()?.quota_list.length === 0}>
                                    <p class="text-sm text-gray-500 text-center py-2">无配额项</p>
                                </Show>
                                <For each={editingStrategy()?.quota_list}>{(item, index) =>
                                    <div class="p-3 border rounded bg-gray-50 space-y-2">
                                        <div class="grid grid-cols-1 md:grid-cols-3 gap-2 items-end">
                                            <div>
                                                <KSelect<QuotaStrategyItemUI['resource_type']>
                                                    value={item.resource_type}
                                                    onChange={(v) => updateQuotaItemField(index(), 'resource_type', v)}
                                                    options={['global', 'provider', 'model']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">资源类型</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <Show when={item.resource_type === 'provider' || item.resource_type === 'model'}>
                                                <TextField value={item.resource_id?.toString() || ''} onChange={v => updateQuotaItemField(index(), 'resource_id', v === '' ? null : parseInt(v))} type="number" min="0">
                                                    <TextField.Label class="form-label form-label-sm">资源ID (Provider/Model ID)</TextField.Label>
                                                    <TextField.Input class="form-input form-input-sm" placeholder="空为所有"/>
                                                </TextField>
                                            </Show>
                                            <Show when={item.resource_type === 'global'}>
                                                <div class="pt-5 text-sm text-gray-400">(全局无需ID)</div>
                                            </Show>

                                            <div>
                                                <KSelect<QuotaStrategyItemUI['limit_type']>
                                                    value={item.limit_type}
                                                    onChange={(v) => updateQuotaItemField(index(), 'limit_type', v)}
                                                    options={['request', 'fee']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">限制类型</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <TextField value={item.limit_value?.toString() || ''} onChange={v => updateQuotaItemField(index(), 'limit_value', v === '' ? null : parseFloat(v))} type="number" min="0" required>
                                                <TextField.Label class="form-label form-label-sm">限制值</TextField.Label>
                                                <TextField.Input class="form-input form-input-sm" />
                                            </TextField>
                                            <div>
                                                <KSelect<QuotaStrategyItemUI['duration']>
                                                    value={item.duration}
                                                    onChange={(v) => updateQuotaItemField(index(), 'duration', v)}
                                                    options={['day', 'hour', 'minute']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class="kobalte-select-item"><KSelect.ItemLabel>{props.item.rawValue}</KSelect.ItemLabel><KSelect.ItemIndicator>✓</KSelect.ItemIndicator></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class="form-label form-label-sm">时长</KSelect.Label>
                                                    <KSelect.Trigger class="form-select form-select-sm w-full"><KSelect.Value<string>>{state => state.selectedOption()}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class="kobalte-select-content"><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            <Button onClick={() => removeItemFromList('quota_list', index())} class="btn btn-danger btn-sm self-end md:col-start-3">删除</Button>
                                        </div>
                                    </div>
                                }</For>
                            </div>
                        </div>


                        {/* Modal Actions */}
                        <div class="form-buttons flex justify-end gap-3 pt-6">
                            <Button onClick={handleCloseModal} class="btn btn-default">取消</Button>
                            <Button onClick={handleSaveStrategy} class="btn btn-primary">保存</Button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    );
}

// Helper styles for Kobalte Select if not globally defined
// You might want to move these to a global CSS file
const style = document.createElement('style');
style.textContent = `
  .kobalte-select-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.375rem 0.75rem; /* py-1.5 px-3 */
    font-size: 0.875rem; /* text-sm */
    color: #374151; /* text-gray-700 */
    cursor: default;
  }
  .kobalte-select-item[data-highlighted] {
    background-color: #DBEAFE; /* bg-blue-100 */
    color: #1D4ED8; /* text-blue-700 */
  }
  .kobalte-select-item[data-selected] {
    font-weight: 600; /* font-semibold */
  }
  .kobalte-select-content {
    background-color: white;
    border: 1px solid #D1D5DB; /* border-gray-300 */
    border-radius: 0.375rem; /* rounded */
    box-shadow: 0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -1px rgba(0,0,0,0.06); /* shadow-lg */
    margin-top: 0.25rem; /* mt-1 */
    z-index: 50;
    max-height: 15rem; /* max-h-60 */
    overflow-y: auto;
  }
  .form-label { display: block; margin-bottom: 0.25rem; font-size: 0.875rem; font-weight: 500; color: #374151; }
  .form-label-sm { font-size: 0.75rem; }
  .form-input, .form-select {
    display: block;
    width: 100%;
    padding: 0.5rem 0.75rem;
    font-size: 1rem;
    line-height: 1.5;
    color: #374151;
    background-color: #fff;
    background-clip: padding-box;
    border: 1px solid #D1D5DB;
    border-radius: 0.375rem;
    transition: border-color .15s ease-in-out,box-shadow .15s ease-in-out;
  }
  .form-input:focus, .form-select:focus-within { /* :focus-within for KSelect.Trigger */
    border-color: #2563EB;
    outline: 0;
    box-shadow: 0 0 0 0.2rem rgba(37,99,235,.25);
  }
  .form-input-sm, .form-select-sm { padding: 0.25rem 0.5rem; font-size: 0.875rem; }

  /* Ensure Kobalte trigger takes full width if needed */
  .kobalte-select__trigger { width: 100%; }
`;
document.head.appendChild(style);
