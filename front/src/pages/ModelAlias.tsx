import { createSignal, For, Show, createResource, createMemo } from 'solid-js';
import { useI18n } from '../i18n'; // Import the i18n hook
import { Button } from '@kobalte/core/button';
import { TextField } from '@kobalte/core/text-field';
import { Select } from '@kobalte/core/select'; // Removed KobalteItemProps import
import { request } from '../services/api';
import { providers as globalProviders } from '../store/providerStore'; // Import from store
import type { ProviderListItem, ModelItem, ModelDetail } from '../store/types'; // Import ModelItem as well

// --- Type Definitions ---

// Represents the raw structure of an item from the API list/detail response
interface ApiModelAliasItem {
    id: number;
    alias_name: string;
    target_model_id: number;
    description: string | null;
    priority: number;
    is_enabled: boolean;
    created_at: number;
    updated_at: number;
    model_name: string; // Per API example, this might be the alias name or related to target.
    // We will primarily use 'real_model_name' for the target's actual name.
    provider_key: string;
    real_model_name: string;
}

// Represents a processed Model Alias for use in the component's state and display
interface ModelAlias {
    id: number;
    alias_name: string;         // The name of the alias (e.g., "gpt-4-alias")
    map_model_name: string;     // Display name for the target model, (e.g., "openai/gpt-4-turbo")
    target_model_id: number;    // ID of the actual target model
    is_enabled: boolean;
    description: string | null; // Description for the alias
    priority: number;           // Priority of the alias
}

// For creating/editing a model alias
interface EditingModelAlias {
    id: number | null;
    alias_name: string; // The name of the alias
    provider_id: number | null; // Selected provider ID for the target model
    target_model_id: number | null; // Selected target model ID
    is_enabled: boolean;
}

const newModelAliasTemplate = (): EditingModelAlias => ({
    id: null,
    alias_name: '',
    provider_id: null,
    target_model_id: null,
    is_enabled: true,
});

// --- API Functions ---
// Removed fetchProvidersWithModelsAPI as we'll use the global store

const fetchModelAliassAPI = async (): Promise<ModelAlias[]> => {
    try {
        const response = await request("/ai/manager/api/model_alias/list");
        const rawAliases: ApiModelAliasItem[] = response || [];

        return rawAliases
            .map((item: ApiModelAliasItem): ModelAlias => ({
                id: item.id,
                alias_name: item.alias_name,
                map_model_name: `${item.provider_key}/${item.model_name}`, // Construct as per requirement
                target_model_id: item.target_model_id,
                is_enabled: item.is_enabled,
                description: item.description,
                priority: item.priority,
            }));
    } catch (error) {
        console.error("Failed to fetch model aliases:", error);
        return [];
    }
};

const fetchModelAliasDetailAPI = async (id: number): Promise<ModelAlias | null> => {
    try {
        const response = await request(`/ai/manager/api/model_alias/${id}`);
        if (!response) return null;

        // Assuming the detail endpoint returns an object similar to ApiModelAliasItem
        const item = response as ApiModelAliasItem;

        return {
            id: item.id,
            alias_name: item.alias_name,
            map_model_name: `${item.provider_key}/${item.real_model_name}`,
            target_model_id: item.target_model_id,
            is_enabled: item.is_enabled,
            description: item.description,
            priority: item.priority,
        };
    } catch (error) {
        console.error("Failed to fetch model alias detail:", error);
        return null;
    }
};

const saveModelAliasAPI = async (transform: EditingModelAlias): Promise<any> => {
    let payload: any;
    if (transform.id) { // Update
        payload = {
            alias_name: transform.alias_name, // alias_name is the alias name
            target_model_id: transform.target_model_id,
            is_enabled: transform.is_enabled,
            // Backend UpdateAliasRequest takes optional fields, so only send what's changed or relevant
        };
    } else { // Create
        payload = {
            alias_name: transform.alias_name, // alias_name is the alias name
            target_model_id: transform.target_model_id,
            is_enabled: transform.is_enabled,
            // description and priority are optional in backend CreateAliasRequest
        };
    }

    const url = transform.id
        ? `/ai/manager/api/model_alias/${transform.id}`
        : '/ai/manager/api/model_alias'; // This is the create endpoint
    const method = transform.id ? 'PUT' : 'POST';

    return request(url, {
        method: method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
    });
};

const deleteModelAliasAPI = async (id: number): Promise<any> => {
    return request(`/ai/manager/api/model_alias/${id}`, { method: 'DELETE' });
};

// --- Component ---
export default function ModelAliasPage() {
    const [transforms, { refetch: refetchTransforms }] = createResource<ModelAlias[]>(fetchModelAliassAPI, { initialValue: [] });
    // Use globalProviders from the store. Assuming ProviderListItem is compatible with local Provider interface.
    // const [providersData] = createResource<Provider[]>(fetchProvidersWithModelsAPI, { initialValue: [] }); // Removed
    const providersData = globalProviders; // Use the imported store resource
    const [showEditModal, setShowEditModal] = createSignal(false);
    const [editingTransform, setEditingTransform] = createSignal<EditingModelAlias>(newModelAliasTemplate());

    // Ensure Provider type here matches ProviderListItem from the store if possible, or is compatible.
    const findProviderForModel = (modelId: number | null, currentProviders: Readonly<ProviderListItem[]> | undefined): number | null => {
        if (modelId === null || !currentProviders) return null;
        for (const pItem of currentProviders) {
            // ProviderListItem has a 'models' array of ModelDetail, and provider info is in pItem.provider
            if (pItem.models && pItem.models.some(m => m.model.id === modelId)) {
                return pItem.provider.id; // Return the id from the nested provider object
            }
        }
        return null;
    };

    const handleOpenAddModal = () => {
        setEditingTransform(newModelAliasTemplate());
        setShowEditModal(true);
    };

    const handleOpenEditModal = async (id: number) => {
        const detail = await fetchModelAliasDetailAPI(id); // detail is ModelAlias
        if (detail && detail.target_model_id) {
            // Pass providersData() which is the signal's value
            const providerId = findProviderForModel(detail.target_model_id, providersData());
            setEditingTransform({
                id: detail.id,
                alias_name: detail.alias_name, // Use alias_name from the fetched detail
                provider_id: providerId,
                target_model_id: detail.target_model_id,
                is_enabled: detail.is_enabled,
            });
            setShowEditModal(true);
        } else {
            alert(t('modelAliasPage.alert.loadDetailFailed'));
        }
    };

    const handleCloseModal = () => {
        setShowEditModal(false);
    };

    const handleSaveTransform = async () => {
        if (!editingTransform()) return;
        if (!editingTransform().alias_name.trim() || editingTransform().target_model_id === null) {
            alert(t('modelAliasPage.alert.nameAndTargetRequired'));
            return;
        }
        try {
            await saveModelAliasAPI(editingTransform());
            setShowEditModal(false);
            refetchTransforms();
        } catch (error) {
            console.error("Failed to save model transform:", error);
            alert(t('modelAliasPage.alert.saveFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
        }
    };

    const handleToggleEnable = async (transform: ModelAlias) => {
        // transform is ModelAlias, which should have target_model_id
        const updatedIsEnabled = !transform.is_enabled;
        try {
            // saveModelAliasAPI expects EditingModelAlias.
            // For toggle, we are essentially performing an update.
            // The backend's UpdateAliasRequest takes optional fields.
            // We construct what's needed for saveModelAliasAPI to build the correct backend payload.
            const providerId = findProviderForModel(transform.target_model_id, providersData());

            const payloadForSave: EditingModelAlias = {
                id: transform.id,
                alias_name: transform.alias_name, // Use alias_name from the ModelAlias object
                provider_id: providerId, // Needed for EditingModelAlias structure
                target_model_id: transform.target_model_id,
                is_enabled: updatedIsEnabled,
            };
            await saveModelAliasAPI(payloadForSave); // saveModelAliasAPI will form the correct PUT payload
            refetchTransforms();
        } catch (error) {
            console.error("Failed to toggle model transform status:", error);
            alert(t('modelAliasPage.alert.toggleFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
        }
    };

    const handleDeleteTransform = async (id: number, name: string) => {
        if (confirm(t('modelAliasPage.confirmDelete', { name: name }))) {
            try {
                await deleteModelAliasAPI(id);
                refetchTransforms();
            } catch (error) {
                console.error("Failed to delete model transform:", error);
                alert(t('modelAliasPage.alert.deleteFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
            }
        }
    };

    const [t] = useI18n();
    return (
        <div class="p-4">
            <div class="flex justify-between items-center mb-6">
                <h1 class="text-2xl font-semibold text-gray-800">{t('modelAliasPage.title')}</h1>
                <Button onClick={handleOpenAddModal} class="btn btn-primary">{t('modelAliasPage.addModelAlias')}</Button>
            </div>

            {/* Data Table */}
            <Show when={transforms.loading}>
                <div class="text-center py-4 text-gray-500">{t('modelAliasPage.loading')}</div>
            </Show>
            <Show when={!transforms.loading && transforms.error}>
                <div class="text-center py-4 text-red-500">{t('modelAliasPage.errorPrefix')}</div>
            </Show>
            <Show when={!transforms.loading && !transforms.error && transforms()?.length === 0}>
                <div class="text-center py-4 text-gray-500">{t('modelAliasPage.noData')}</div>
            </Show>
            <Show when={!transforms.loading && !transforms.error && transforms() && transforms()!.length > 0}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <table class="min-w-full divide-y divide-gray-200 data-table">
                        <thead class="bg-gray-100">
                            <tr>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('modelAliasPage.table.aliasName')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('modelAliasPage.table.targetModelName')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('modelAliasPage.table.enabled')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('actions')}</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">
                            <For each={transforms()}>{(transform) =>
                                <tr class="hover:bg-gray-50">
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800">{transform.alias_name}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{transform.map_model_name}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">
                                        <input
                                            type="checkbox"
                                            class="form-checkbox" // Use existing checkbox style
                                            checked={transform.is_enabled}
                                            onChange={() => handleToggleEnable(transform)}
                                        // Add a unique id for accessibility if needed, e.g., `id={`enable-transform-${transform.id}`}`
                                        // For now, direct onChange is sufficient for functionality.
                                        />
                                    </td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm space-x-2">
                                        <Button onClick={() => handleOpenEditModal(transform.id)} class="btn btn-primary btn-sm">{t('edit')}</Button>
                                        <Button onClick={() => handleDeleteTransform(transform.id, transform.alias_name)} class="btn btn-danger btn-sm">{t('delete')}</Button>
                                    </td>
                                </tr>
                            }</For>
                        </tbody>
                    </table>
                </div>
            </Show>

            {/* Edit/Add Modal */}
            <Show when={showEditModal()}>
                <div class="fixed inset-0 bg-gray-500 bg-opacity-75 transition-opacity z-40 model-mask" onClick={handleCloseModal}></div>
                <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
                    <div class="bg-white rounded-lg shadow-xl p-6 space-y-4 w-full max-w-lg model" style="height: auto;"> {/* Adjusted max-w and height */}
                        <h2 class="text-xl font-semibold text-gray-800 model-title mb-6">
                            {editingTransform()?.id ? t('modelAliasPage.modal.titleEdit') : t('modelAliasPage.modal.titleAdd')}
                        </h2>

                        <TextField class="form-item" value={editingTransform()?.alias_name || ''} onChange={(v) => setEditingTransform(s => ({ ...s!, alias_name: v }))}>
                            <TextField.Label class="form-label">{t('modelAliasPage.modal.labelAliasName')}</TextField.Label>
                            <TextField.Input class="form-input" placeholder={t('modelAliasPage.modal.placeholderAliasName')} />
                        </TextField>

                        {/* Provider Select */}
                        <div class="form-item">
                            <Select<ProviderListItem>
                                value={providersData()?.find(pItem => pItem.provider.id === editingTransform().provider_id)}
                                onChange={(selectedPItem: ProviderListItem | null) => {
                                    setEditingTransform(s => ({ ...s!, provider_id: selectedPItem?.provider.id ?? null, target_model_id: null }));
                                }}
                                options={providersData() || []}
                                optionValue={item => item.provider.id} // Access nested id
                                optionTextValue={item => item.provider.name} // Access nested name
                                placeholder={t('modelAliasPage.modal.placeholderProvider')}
                                itemComponent={props => ( // Removed explicit props typing
                                    <Select.Item item={props.item} class="select__item p-2 hover:bg-gray-100 cursor-pointer">
                                        {/* props.item.rawValue is ProviderListItem, so access props.item.rawValue.provider.name */}
                                        {/* If props.item.rawValue is unknown, use: (props.item.rawValue as ProviderListItem).provider.name */}
                                        <Select.ItemLabel>{(props.item.rawValue as ProviderListItem).provider.name}</Select.ItemLabel>
                                    </Select.Item>
                                )}
                            >
                                <Select.Label class="form-label">{t('modelAliasPage.modal.labelTargetProvider')}</Select.Label>
                                <Select.Trigger class="form-input w-full flex justify-between items-center" aria-label="Provider">
                                    <Select.Value<ProviderListItem>>
                                        {(state) => {
                                            const selected = state.selectedOption(); // selected is ProviderListItem | undefined
                                            return selected ? selected.provider.name : <span class="text-gray-500">{t('modelAliasPage.modal.placeholderProvider')}</span>;
                                        }}
                                    </Select.Value>
                                    <Select.Icon class="select__icon">▼</Select.Icon>
                                </Select.Trigger>
                                <Select.Portal>
                                    <Select.Content class="select__content bg-white border border-gray-300 rounded-md shadow-lg mt-1 z-50">
                                        <Select.Listbox class="select__listbox p-1 max-h-60 overflow-y-auto" />
                                    </Select.Content>
                                </Select.Portal>
                            </Select>
                        </div>

                        {/* Model Select (dependent on selectedProviderId) */}
                        <Show when={editingTransform()?.provider_id !== null && (providersData.loading === false || providersData() !== undefined)}>
                            <div class="form-item">
                                <Select
                                    value={
                                        createMemo(() => {
                                            const pid = editingTransform()?.provider_id;
                                            const tid = editingTransform()?.target_model_id;
                                            if (!pid || !providersData() || tid === null) return undefined;
                                            const providerItem = providersData()!.find(pItem => pItem.provider.id === pid);
                                            return providerItem?.models?.find(m => m.model.id === tid);
                                        })()
                                    }
                                    onChange={(selectedModel: ModelDetail | null) => {
                                        setEditingTransform(s => ({ ...s!, target_model_id: selectedModel?.model.id ?? null }));
                                    }}
                                    options={
                                        createMemo(() => {
                                            const pid = editingTransform()?.provider_id;
                                            if (!pid || !providersData()) return [];
                                            const providerItem = providersData()!.find(pItem => pItem.provider.id === pid);
                                            return providerItem && providerItem.models ? providerItem.models : [];
                                        })()
                                    }
                                    optionValue={item => item.model.id} // ModelDetail has 'model' object
                                    optionTextValue={item => item.model.model_name} // ModelDetail has 'model' object
                                    placeholder={t('modelAliasPage.modal.placeholderModel')}
                                    disabled={editingTransform()?.provider_id === null}
                                    itemComponent={props => ( // Removed explicit props typing
                                        <Select.Item item={props.item} class="select__item p-2 hover:bg-gray-100 cursor-pointer">
                                            {/* props.item.rawValue is ModelDetail, access props.item.rawValue.model.model_name */}
                                            <Select.ItemLabel>{(props.item.rawValue as ModelDetail).model.model_name}</Select.ItemLabel>
                                        </Select.Item>
                                    )}
                                >
                                    <Select.Label class="form-label">{t('modelAliasPage.modal.labelTargetModel')}</Select.Label>
                                    <Select.Trigger class="form-input w-full flex justify-between items-center" aria-label="Model">
                                        <Select.Value<ModelDetail>>
                                            {(state) => {
                                                const selected = state.selectedOption(); // selected is ModelDetail | undefined
                                                return selected ? selected.model.model_name : <span class="text-gray-500">{t('modelAliasPage.modal.placeholderModel')}</span>;
                                            }}
                                        </Select.Value>
                                        <Select.Icon class="select__icon">▼</Select.Icon>
                                    </Select.Trigger>
                                    <Select.Portal>
                                        <Select.Content class="select__content bg-white border border-gray-300 rounded-md shadow-lg mt-1 z-50">
                                            <Select.Listbox class="select__listbox p-1 max-h-60 overflow-y-auto" />
                                        </Select.Content>
                                    </Select.Portal>
                                </Select>
                            </div>
                        </Show>


                        <div class="form-item"> {/* Retain form-item for flex layout, label has own margin */}
                            <label for="is_enabled_checkbox" class="form-label">{t('modelAliasPage.modal.labelEnabled')}</label> {/* Changed to label and associated with checkbox */}
                            <input
                                type="checkbox"
                                id="is_enabled_checkbox"
                                class="form-checkbox" /* Style from utilities.css, will be adjusted */
                                checked={editingTransform()?.is_enabled || false}
                                onChange={(e) => setEditingTransform(s => ({ ...s!, is_enabled: e.currentTarget.checked }))}
                            />
                        </div>

                        <div class="form-buttons flex justify-end gap-3 pt-4"> {/* Ensured form-buttons class is present */}
                            <Button onClick={handleCloseModal} class="btn btn-default">{t('common.cancel')}</Button>
                            <Button onClick={handleSaveTransform} class="btn btn-primary">{t('common.save')}</Button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    );
}
