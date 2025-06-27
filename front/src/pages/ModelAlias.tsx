import { createSignal, For, Show, createResource, createMemo } from 'solid-js';
import { useI18n } from '../i18n'; // Import the i18n hook
import { Button } from '../components/ui/Button';
import { TextField } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import {
    DialogRoot,
    DialogContent,
    DialogHeader,
    DialogFooter,
    DialogTitle,
} from '../components/ui/Dialog';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '../components/ui/Table';
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

    const providerOptions = createMemo(() => (providersData() || []).map(p => ({ value: p.provider.id, label: p.provider.name })));

    const modelMap = createMemo(() => {
        const map = new Map<number, { value: number; label: string; }[]>();
        const providers = providersData();
        if (!providers) return map;

        for (const providerItem of providers) {
            const models = (providerItem.models || []).map(m => ({ value: m.model.id, label: m.model.model_name }));
            map.set(providerItem.provider.id, models);
        }
        return map;
    });

    const modelOptions = createMemo(() => {
        const pid = editingTransform()?.provider_id;
        if (!pid) return [];
        return modelMap().get(pid) || [];
    });

    const selectedProvider = createMemo(() => providerOptions().find(p => p.value === editingTransform().provider_id));
    const selectedModel = createMemo(() => modelOptions().find(m => m.value === editingTransform().target_model_id));

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
                <Button onClick={handleOpenAddModal} variant="primary">{t('modelAliasPage.addModelAlias')}</Button>
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
                    <TableRoot>
                        <TableHeader>
                            <TableRow>
                                <TableColumnHeader>{t('modelAliasPage.table.aliasName')}</TableColumnHeader>
                                <TableColumnHeader>{t('modelAliasPage.table.targetModelName')}</TableColumnHeader>
                                <TableColumnHeader>{t('modelAliasPage.table.enabled')}</TableColumnHeader>
                                <TableColumnHeader>{t('actions')}</TableColumnHeader>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            <For each={transforms()}>{(transform) =>
                                <TableRow>
                                    <TableCell>{transform.alias_name}</TableCell>
                                    <TableCell>{transform.map_model_name}</TableCell>
                                    <TableCell>
                                        <input
                                            type="checkbox"
                                            class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                                            checked={transform.is_enabled}
                                            onChange={() => handleToggleEnable(transform)}
                                        />
                                    </TableCell>
                                    <TableCell class="space-x-2">
                                        <Button onClick={() => handleOpenEditModal(transform.id)} variant="primary" size="sm">{t('edit')}</Button>
                                        <Button onClick={() => handleDeleteTransform(transform.id, transform.alias_name)} variant="destructive" size="sm">{t('delete')}</Button>
                                    </TableCell>
                                </TableRow>
                            }</For>
                        </TableBody>
                    </TableRoot>
                </div>
            </Show>

            {/* Edit/Add Modal */}
            <DialogRoot open={showEditModal()} onOpenChange={setShowEditModal}>
                <DialogContent class="max-w-lg">
                    <DialogHeader>
                        <DialogTitle>
                            {editingTransform()?.id ? t('modelAliasPage.modal.titleEdit') : t('modelAliasPage.modal.titleAdd')}
                        </DialogTitle>
                    </DialogHeader>
                    <div class="space-y-4">
                        <TextField
                            label={t('modelAliasPage.modal.labelAliasName')}
                            placeholder={t('modelAliasPage.modal.placeholderAliasName')}
                            value={editingTransform()?.alias_name || ''}
                            onChange={(v) => setEditingTransform(s => ({ ...s!, alias_name: v }))}
                        />

                        <Select
                            label={t('modelAliasPage.modal.labelTargetProvider')}
                            placeholder={t('modelAliasPage.modal.placeholderProvider')}
                            value={selectedProvider()}
                            options={providerOptions()}
                            optionValue="value"
                            optionTextValue="label"
                            onChange={(v) => {
                                setEditingTransform(s => ({ ...s!, provider_id: v ? v.value : null, target_model_id: null }));
                            }}
                        />

                        <Show when={editingTransform()?.provider_id !== null}>
                            <Select
                                label={t('modelAliasPage.modal.labelTargetModel')}
                                placeholder={t('modelAliasPage.modal.placeholderModel')}
                                value={selectedModel()}
                                options={modelOptions()}
                                optionValue="value"
                                optionTextValue="label"
                                onChange={(v) => {
                                    setEditingTransform(s => ({ ...s!, target_model_id: v ? v.value : null }));
                                }}
                                disabled={editingTransform()?.provider_id === null}
                            />
                        </Show>


                        <div class="flex items-center space-x-2 pt-2">
                            <label for="is_enabled_checkbox" class="text-sm font-medium leading-none">{t('modelAliasPage.modal.labelEnabled')}</label>
                            <input
                                type="checkbox"
                                id="is_enabled_checkbox"
                                class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                                checked={editingTransform()?.is_enabled || false}
                                onChange={(e) => setEditingTransform(s => ({ ...s!, is_enabled: e.currentTarget.checked }))}
                            />
                        </div>
                    </div>
                    <DialogFooter class="pt-4">
                        <Button onClick={handleCloseModal} variant="secondary">{t('common.cancel')}</Button>
                        <Button onClick={handleSaveTransform} variant="primary">{t('common.save')}</Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>
        </div>
    );
}
