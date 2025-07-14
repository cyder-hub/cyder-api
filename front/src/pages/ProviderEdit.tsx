import { For, Show, onMount, createSignal, createMemo } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import { useI18n } from '../i18n';
import { Button } from '../components/ui/Button';
import { TextField } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { useNavigate, useParams } from '@solidjs/router';
import { request } from '../services/api';
import { refetchProviders as globalRefetchProviders } from '../store/providerStore';
import { toastController } from '../components/GlobalMessage';
import type { ProviderListItem, CustomFieldType, ProviderApiKeyItem as BackendProviderApiKeyItem } from '../store/types';

// Local interface for custom fields, must include id for linking/unlinking
interface CustomFieldItem {
    id: number;
    field_name: string;
    field_value: string;
    description: string | null;
    field_type: CustomFieldType;
}

// Local interface for provider API keys within this component
interface LocalProviderApiKeyItem {
    id?: number | null;
    api_key: string;
    description: string | null;
    isEditing: boolean;
    checkStatus: 'unchecked' | 'checking' | 'success' | 'error';
    checkMessage?: string;
}

// Local interface for editable models within this component
interface LocalEditableModelItem {
    id?: number | null;
    model_name: string;
    real_model_name: string | null; // Match backend Option<String>
    isEditing: boolean;
    checkStatus: 'unchecked' | 'checking' | 'success' | 'error';
    checkMessage?: string;
    // Add is_enabled if it needs to be managed directly, default to true for new
}

// Interface for the data being edited
interface EditingProviderData {
    id: number | null;
    name: string;
    provider_key: string;
    provider_type: string;
    endpoint: string;
    use_proxy: boolean;
    models: LocalEditableModelItem[]; // Use local interface
    provider_keys: LocalProviderApiKeyItem[]; // Use local interface
    custom_fields: CustomFieldItem[];
}

const fetchProviderDetail = async (providerId: number): Promise<ProviderListItem | null> => {
    try {
        const response = await request(`/ai/manager/api/provider/${providerId}/detail`);
        return response || null;
    } catch (error) {
        console.error(t('providerEditPage.alert.fetchDetailFailed', { providerId: providerId }), error);
        return null;
    }
};

const fetchAllCustomFields = async (): Promise<CustomFieldItem[]> => {
    try {
        const response = await request('/ai/manager/api/custom_field_definition/list');
        if (response && response.list) {
            return response.list.map((f: any) => ({
                id: f.id,
                field_name: f.field_name,
                field_value: (f.string_value ?? f.integer_value?.toString() ?? f.number_value?.toString() ?? f.boolean_value?.toString()) || '',
                description: f.description,
                field_type: (f.field_type?.toLowerCase() as CustomFieldType) || 'unset'
            }));
        }
        return [];
    } catch (error) {
        console.error('Failed to fetch all custom fields', error);
        toastController.error('Failed to fetch all custom fields');
        return [];
    }
};

const providerTypes = ['OPENAI', 'GEMINI', 'VERTEX', 'VERTEX_OPENAI', 'OLLAMA'];
const customFieldTypes: CustomFieldType[] = ['unset', 'text', 'integer', 'float', 'boolean'];

const StatusIndicator = (props: { status: 'unchecked' | 'checking' | 'success' | 'error', message?: string }) => {
    const statusColor = {
        unchecked: 'bg-gray-400',
        checking: 'bg-blue-500 animate-pulse',
        success: 'bg-green-500',
        error: 'bg-red-500',
    };
    return (
        <div class="relative group flex justify-center items-center h-full">
            <div class={`w-3 h-3 rounded-full ${statusColor[props.status]}`}></div>
            <Show when={props.status === 'error' && props.message}>
                <div class="absolute bottom-full mb-2 w-max max-w-xs px-2 py-1 bg-gray-800 text-white text-xs rounded opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none z-10">
                    {props.message}
                </div>
            </Show>
        </div>
    );
};

export default function ProviderEdit() {
    const navigate = useNavigate();
    const params = useParams();
    const providerId = params.id ? parseInt(params.id) : null;
    const [t] = useI18n();

    // Use createStore instead of createSignal for editingData
    const [editingData, setEditingData] = createStore<EditingProviderData | null>(null);
    const [allCustomFields, setAllCustomFields] = createSignal<CustomFieldItem[]>([]);
    const [selectedCustomFieldId, setSelectedCustomFieldId] = createSignal<number | null>(null);
    const [isModelSelectModalOpen, setIsModelSelectModalOpen] = createSignal(false);
    const [apiKeyIndexToCheck, setApiKeyIndexToCheck] = createSignal<number | null>(null);
    const [modelIndexToUse, setModelIndexToUse] = createSignal<number | null>(null);
    const [isBatchCheckingApiKeys, setIsBatchCheckingApiKeys] = createSignal(false);

    const [isApiKeySelectModalOpen, setIsApiKeySelectModalOpen] = createSignal(false);
    const [modelIndexToCheck, setModelIndexToCheck] = createSignal<number | null>(null);
    const [apiKeyIndexToUse, setApiKeyIndexToUse] = createSignal<number | null>(null);
    const [isBatchCheckingModels, setIsBatchCheckingModels] = createSignal(false);
    // setIsLoading and setError can remain createSignal as they are simple booleans/strings
    const [isLoading, setIsLoading] = createSignal<boolean>(true);
    const [error, setError] = createSignal<string | null>(null);

    const hasUncommittedModels = createMemo(() => {
        if (!editingData) return false;
        return editingData.models?.some(m => m.id === null);
    });

    const modelOptionsForSelect = createMemo(() => {
        if (!editingData?.models) return [];
        return editingData.models.map((m, i) => ({ value: i, label: m.model_name }));
    });

    const apiKeyOptionsForSelect = createMemo(() => {
        if (!editingData?.provider_keys) return [];
        return editingData.provider_keys.map((k, i) => ({
            value: i,
            label: k.description || t('providerEditPage.alert.apiKeyNameFallback', { lastKeyChars: k.api_key.slice(-4) })
        }));
    });

    const getEmptyProvider = (): EditingProviderData => ({
        id: null,
        name: '',
        provider_key: '',
        provider_type: 'OPENAI',
        endpoint: '',
        use_proxy: false,
        models: [],
        provider_keys: [],
        custom_fields: [],
    });

    onMount(async () => {
        setIsLoading(true);
        setError(null);

        const fields = await fetchAllCustomFields();
        setAllCustomFields(fields);

        if (providerId) {
            const detail = await fetchProviderDetail(providerId);
            if (detail) {
                setEditingData({
                    id: detail.provider.id,
                    name: detail.provider.name,
                    provider_key: detail.provider.provider_key,
                    provider_type: detail.provider.provider_type || 'OPENAI',
                    endpoint: detail.provider.endpoint,
                    use_proxy: detail.provider.use_proxy,
                    models: detail.models.map(m => ({ // Map to LocalEditableModelItem
                        id: m.model.id,
                        model_name: m.model.model_name,
                        real_model_name: m.model.real_model_name ?? null,
                        isEditing: false,
                        checkStatus: 'unchecked',
                    })),
                    provider_keys: detail.provider_keys.map(k => ({
                        id: k.id,
                        api_key: k.api_key,
                        description: k.description ?? null,
                        isEditing: false, // Initialize isEditing state
                        checkStatus: 'unchecked',
                    })),
                    custom_fields: (detail.custom_fields || []).map(f => ({
                        id: f.id,
                        field_name: f.field_name,
                        field_value: (f.string_value ?? f.integer_value?.toString() ?? f.number_value?.toString() ?? f.boolean_value?.toString()) || '',
                        description: f.description,
                        field_type: (f.field_type?.toLowerCase() as CustomFieldType) || 'unset'
                    })),
                });
            } else {
                setError(t('providerEditPage.alert.loadDataFailed', { providerId: providerId! }));
            }
        } else {
            setEditingData(getEmptyProvider());
        }
        setIsLoading(false);
    });

    // This function is kept for the "Cancel" button that navigates away from the page.
    const handleNavigateBack = () => {
        navigate('/provider');
    };

    // Removed handleCommitProvider function

    const handleUpdateProviderBaseInfo = async () => {
        const currentData = editingData;
        if (!currentData) return;

        // Validation for base fields
        if (!currentData.name.trim()) { toastController.warn(t('providerEditPage.alert.nameRequired')); return; }
        if (!currentData.provider_key.trim()) { toastController.warn(t('providerEditPage.alert.providerKeyRequired')); return; }
        if (!currentData.endpoint.trim()) { toastController.warn(t('providerEditPage.alert.endpointRequired')); return; }

        const basePayload = {
            key: currentData.provider_key, // Matches InserPayload 'key' field
            name: currentData.name,
            endpoint: currentData.endpoint,
            use_proxy: currentData.use_proxy,
            provider_type: currentData.provider_type,
            omit_config: null, // Assuming no omit_config management in this UI for now
            api_keys: [], // Send empty as InserPayload requires it, but Provider::update_one won't use it to modify keys.
        };

        try {
            if (currentData.id) { // Existing provider, PUT to update
                await request(`/ai/manager/api/provider/${currentData.id}`, {
                    method: 'PUT',
                    body: JSON.stringify(basePayload)
                });
                toastController.success(t('providerEditPage.alert.baseInfoUpdateSuccess'));
            } else { // New provider, POST to create
                const newProvider = await request<ProviderBase>(`/ai/manager/api/provider`, { // Expect ProviderBase or similar with ID
                    method: 'POST',
                    body: JSON.stringify(basePayload)
                });
                // Update local store with the new ID so subsequent model/key additions work
                setEditingData('id', newProvider.id);
                toastController.success(t('providerEditPage.alert.createSuccess'));
            }
            globalRefetchProviders(); // Refetch provider list on main page
        } catch (error) {
            console.error("Failed to save provider base info:", error);
            toastController.error(t('providerEditPage.alert.baseInfoSaveFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const updateEditingDataField = (field: keyof EditingProviderData, value: any) => {
        // Direct path update with createStore
        if (editingData) { // Ensure editingData is not null
            setEditingData(field, value);
        }
    };

    const handleSaveSingleApiKey = async (index: number) => {
        const currentData = editingData; // Changed from editingData()
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForApiKey'));
            return;
        }

        const keyItem = currentData.provider_keys[index];
        if (!keyItem.api_key.trim()) {
            toastController.warn(t('providerEditPage.alert.apiKeyRequiredWithIndex', { index: index + 1 }));
            return;
        }

        if (currentData.provider_type === 'VERTEX') {
            try {
                const parsedKey = JSON.parse(keyItem.api_key);
                const requiredFields = ['client_email', 'private_key', 'private_key_id', 'token_uri'];
                const missingFields = requiredFields.filter(field => !(field in parsedKey) || !parsedKey[field]);
                if (missingFields.length > 0) {
                    toastController.warn(t('providerEditPage.alert.vertexApiKeyMissingFields', { index: index + 1, fields: missingFields.join(', ') }));
                    return;
                }
            } catch (e) {
                toastController.warn(t('providerEditPage.alert.vertexApiKeyInvalidJson', { index: index + 1 }));
                return;
            }
        }

        try {
            const savedKey = await request<BackendProviderApiKeyItem>(`/ai/manager/api/provider/${currentData.id}/provider_key`, {
                method: 'POST',
                body: JSON.stringify({
                    api_key: keyItem.api_key,
                    description: keyItem.description,
                }),
            });

            // Update using store path syntax
            setEditingData('provider_keys', index, {
                // It's often better to update specific fields if not all are changing
                // or merge with existing if `savedKey` is partial.
                // Assuming savedKey contains all necessary fields for LocalProviderApiKeyItem:
                id: savedKey.id,
                api_key: savedKey.api_key,
                description: savedKey.description ?? null,
                isEditing: false, // Turn off editing mode after save
            });
            toastController.success(t('providerEditPage.alert.apiKeySaveSuccess'));
        } catch (error) {
            console.error("Failed to save API key:", error);
            toastController.error(t('providerEditPage.alert.saveApiKeyFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const addListItem = <T extends { id?: number | null; isEditing?: boolean; checkStatus?: 'unchecked' | 'checking' | 'success' | 'error' }>(
        listField: 'models' | 'provider_keys',
        newItemData: Omit<T, 'id' | 'isEditing' | 'checkStatus'>
    ) => {
        let itemToAdd: T;
        if (listField === 'provider_keys') {
            itemToAdd = { ...newItemData, id: null, isEditing: false, checkStatus: 'unchecked' } as T;
        } else if (listField === 'models') {
            itemToAdd = { ...newItemData, id: null, isEditing: false, checkStatus: 'unchecked' } as T; // New models are not in edit mode by default
        } else {
            return;
        }

        setEditingData(
            listField as any,
            produce((list: T[]) => {
                list.push(itemToAdd);
            })
        );
    };

    const handleEditApiKey = (index: number) => {
        setEditingData('provider_keys', index, 'isEditing', true);
    };

    const handleCancelEditApiKey = (index: number) => {
        // Potentially revert changes here if original data was stored temporarily
        setEditingData('provider_keys', index, 'isEditing', false);
    };

    const handleSaveSingleModel = async (index: number) => {
        const currentProviderData = editingData;
        if (!currentProviderData || !currentProviderData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForModel'));
            return;
        }

        const modelItem = currentProviderData.models[index];
        if (!modelItem.model_name.trim()) {
            toastController.warn(t('providerEditPage.alert.modelIdRequiredWithIndex', { index: index + 1 }));
            return;
        }

        try {
            // This function now only handles creating new models.
            const savedModel = await request<ModelItem>('/ai/manager/api/model', {
                method: 'POST',
                body: JSON.stringify({
                    provider_id: currentProviderData.id,
                    model_name: modelItem.model_name,
                    real_model_name: modelItem.real_model_name || null,
                    is_enabled: true,  // Default for new
                }),
            });

            setEditingData('models', index, {
                id: savedModel.id,
                model_name: savedModel.model_name,
                real_model_name: (savedModel as any).real_model_name ?? null, // Cast if BackendProviderApiKeyItem doesn't fit perfectly
                isEditing: false,
            });
            toastController.success(t('providerEditPage.alert.modelSaveSuccess'));
        } catch (error) {
            console.error("Failed to save model:", error);
            toastController.error(t('providerEditPage.alert.saveModelFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const handleDeleteModel = async (index: number) => {
        const currentProviderData = editingData;
        if (!currentProviderData) return;

        const modelItem = currentProviderData.models[index];

        if (modelItem.id) { // Existing model, delete from backend
            if (!currentProviderData.id) { // Should not happen if modelItem.id exists, but good check
                toastController.warn(t('providerEditPage.alert.providerNotSavedForModelDelete'));
                return;
            }
            try {
                await request(`/ai/manager/api/model/${modelItem.id}`, {
                    method: 'DELETE',
                });
                // On successful deletion from backend, remove from local state
                setEditingData(
                    'models',
                    produce((models: LocalEditableModelItem[]) => {
                        models.splice(index, 1);
                    })
                );
                toastController.success(t('providerEditPage.alert.modelDeleteSuccess'));
            } catch (error) {
                console.error("Failed to delete model:", error);
                toastController.error(t('providerEditPage.alert.deleteModelFailed', { error: (error as Error).message || t('unknownError') }));
                return;
            }
        } else { // New model, just remove locally
            setEditingData(
                'models',
                produce((models: LocalEditableModelItem[]) => {
                    models.splice(index, 1);
                })
            );
        }
    };

    const handleFetchRemoteModels = async () => {
        const currentData = editingData;
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForModel'));
            return;
        }

        try {
            const response = await request(`/ai/manager/api/provider/${currentData.id}/remote_models`);

            let remoteModels: any[] = [];
            let isGeminiLike = false;
            if (response) {
                if (Array.isArray(response.data)) { // OpenAI-like
                    remoteModels = response.data;
                } else if (Array.isArray(response.models)) { // Google Gemini-like
                    remoteModels = response.models;
                    isGeminiLike = true;
                } else if (Array.isArray(response)) { // Direct array response
                    remoteModels = response;
                }
            }

            if (!remoteModels || remoteModels.length === 0) {
                toastController.warn(t('providerEditPage.alert.noRemoteModels'));
                return;
            }

            const existingModelNames = new Set<string>();
            currentData.models.forEach(m => {
                existingModelNames.add(m.model_name);
                if (m.real_model_name) {
                    existingModelNames.add(m.real_model_name);
                }
            });

            const newModels: LocalEditableModelItem[] = [];
            remoteModels.forEach(item => {
                let model_name = item.id || item.name;
                const providerType = currentData.provider_type;
                const isGoogleOwned = item.owned_by === 'google';
                const isGeminiProvider = providerType === 'GEMINI' || providerType === 'VERTEX';

                if ((isGeminiProvider || isGeminiLike || isGoogleOwned) && model_name && model_name.startsWith('models/')) {
                    model_name = model_name.substring('models/'.length);
                }

                if (model_name && !existingModelNames.has(model_name)) {
                    newModels.push({
                        id: null,
                        model_name: model_name,
                        real_model_name: null,
                        isEditing: false,
                        checkStatus: 'unchecked',
                    });
                    existingModelNames.add(model_name); // Add to set to avoid duplicates from remote list
                }
            });

            if (newModels.length > 0) {
                setEditingData('models', produce(models => {
                    models.push(...newModels);
                }));
                toastController.success(t('providerEditPage.alert.newModelsAdded', { count: newModels.length }));
            } else {
                toastController.info(t('providerEditPage.alert.noNewModels'));
            }

        } catch (error) {
            console.error("Failed to fetch remote models:", error);
            toastController.error(t('providerEditPage.alert.fetchRemoteModelsFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const handleClearUncommittedModels = () => {
        if (!editingData) return;
        const originalCount = editingData.models.length;
        const committedModels = editingData.models.filter(m => m.id !== null);
        if (originalCount > committedModels.length) {
            setEditingData('models', committedModels);
            toastController.info(t('providerEditPage.alert.uncommittedCleared'));
        } else {
            toastController.info(t('providerEditPage.alert.noUncommittedToClear'));
        }
    };

    const availableCustomFields = () => {
        if (!editingData) return [];
        const linkedIds = new Set(editingData.custom_fields.map(f => f.id));
        return allCustomFields().filter(f => f.id && !linkedIds.has(f.id));
    };

    const handleLinkCustomField = async () => {
        const field = selectedCustomFieldId();
        const providerId = editingData?.id;

        if (!field) {
            toastController.warn(t('providerEditPage.alert.selectCustomField'));
            return;
        }
        if (!providerId) {
            toastController.warn(t('providerEditPage.alert.saveProviderBeforeLink'));
            return;
        }

        // The Select component may return the whole object. Extract the id for the API call.
        const fieldId = (field as any).id ?? field;

        try {
            await request('/ai/manager/api/custom_field_definition/link', {
                method: 'POST',
                body: JSON.stringify({
                    custom_field_definition_id: fieldId,
                    provider_id: providerId,
                    is_enabled: true,
                }),
            });

            const fieldToAdd = allCustomFields().find(f => f.id === fieldId);
            if (fieldToAdd) {
                setEditingData('custom_fields', produce(fields => {
                    fields.push(fieldToAdd);
                }));
            }
            setSelectedCustomFieldId(null);
            toastController.success(t('providerEditPage.alert.linkCustomFieldSuccess'));
        } catch (error) {
            console.error("Failed to link custom field:", error);
            toastController.error(t('providerEditPage.alert.linkCustomFieldFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const handleUnlinkCustomField = async (fieldId: number, index: number) => {
        const providerId = editingData?.id;
        if (!providerId) {
            toastController.warn(t('providerEditPage.alert.providerIdNotFound'));
            return;
        }

        try {
            await request('/ai/manager/api/custom_field_definition/unlink', {
                method: 'POST',
                body: JSON.stringify({
                    custom_field_definition_id: fieldId,
                    provider_id: providerId,
                }),
            });

            setEditingData('custom_fields', produce(fields => {
                fields.splice(index, 1);
            }));
            toastController.success(t('providerEditPage.alert.unlinkCustomFieldSuccess'));
        } catch (error) {
            console.error("Failed to unlink custom field:", error);
            toastController.error(t('providerEditPage.alert.unlinkedCustomFieldFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const performCheck = async (modelIndex: number, apiKeyIndex: number) => {
        const currentData = editingData;
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForCheck'));
            return;
        }

        // Set both to checking
        setEditingData('models', modelIndex, 'checkStatus', 'checking');
        setEditingData('models', modelIndex, 'checkMessage', undefined);
        setEditingData('provider_keys', apiKeyIndex, 'checkStatus', 'checking');
        setEditingData('provider_keys', apiKeyIndex, 'checkMessage', undefined);

        const modelItem = currentData.models[modelIndex];
        const keyItem = currentData.provider_keys[apiKeyIndex];

        const payload = {
            ...(modelItem.id ? { model_id: modelItem.id } : { model_name: modelItem.real_model_name || modelItem.model_name }),
            ...(keyItem.id ? { provider_api_key_id: keyItem.id } : { provider_api_key: keyItem.api_key }),
        };

        try {
            await request(`/ai/manager/api/provider/${currentData.id}/check`, {
                method: 'POST',
                body: JSON.stringify(payload),
            });
            // On success, update both
            setEditingData('models', modelIndex, 'checkStatus', 'success');
            setEditingData('provider_keys', apiKeyIndex, 'checkStatus', 'success');
        } catch (error) {
            const errorMessage = (error as Error).message || t('unknownError');
            setEditingData('models', modelIndex, 'checkStatus', 'error');
            setEditingData('models', modelIndex, 'checkMessage', errorMessage);
            setEditingData('provider_keys', apiKeyIndex, 'checkStatus', 'error');
            setEditingData('provider_keys', apiKeyIndex, 'checkMessage', errorMessage);
        }
    };

    const performBatchModelCheck = async (apiKeyIndex: number) => {
        const currentData = editingData;
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForCheck'));
            return;
        }

        const providerId = currentData.id;
        const translatedType = t('providerEditPage.alert.checkTypeModels');
        toastController.info(t('providerEditPage.alert.batchChecking', { type: translatedType }));

        const modelsToCheck = currentData.models;
        const key = currentData.provider_keys[apiKeyIndex];

        modelsToCheck.forEach((_, index) => {
            setEditingData('models', index, 'checkStatus', 'checking');
            setEditingData('models', index, 'checkMessage', undefined);
        });

        let successCount = 0;
        for (const [index, model] of modelsToCheck.entries()) {
            const payload = {
                ...(model.id ? { model_id: model.id } : { model_name: model.real_model_name || model.model_name }),
                ...(key.id ? { provider_api_key_id: key.id } : { provider_api_key: key.api_key }),
            };
            try {
                await request(`/ai/manager/api/provider/${providerId}/check`, {
                    method: 'POST',
                    body: JSON.stringify(payload),
                });
                successCount++;
                setEditingData('models', index, 'checkStatus', 'success');
            } catch (error) {
                const errorMessage = (error as Error).message || t('unknownError');
                setEditingData('models', index, 'checkStatus', 'error');
                setEditingData('models', index, 'checkMessage', errorMessage);
            }
        }
        toastController.info(t('providerEditPage.alert.batchCheckComplete', { success: successCount, total: modelsToCheck.length, type: translatedType }));
    };

    const performBatchApiKeyCheck = async (modelIndex: number) => {
        const currentData = editingData;
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForCheck'));
            return;
        }

        const providerId = currentData.id;
        const translatedType = t('providerEditPage.alert.checkTypeApiKeys');
        toastController.info(t('providerEditPage.alert.batchChecking', { type: translatedType }));

        const keysToCheck = currentData.provider_keys;
        const model = currentData.models[modelIndex];

        keysToCheck.forEach((_, index) => {
            setEditingData('provider_keys', index, 'checkStatus', 'checking');
            setEditingData('provider_keys', index, 'checkMessage', undefined);
        });

        let successCount = 0;
        for (const [index, key] of keysToCheck.entries()) {
            const payload = {
                ...(model.id ? { model_id: model.id } : { model_name: model.real_model_name || model.model_name }),
                ...(key.id ? { provider_api_key_id: key.id } : { provider_api_key: key.api_key }),
            };
            try {
                await request(`/ai/manager/api/provider/${providerId}/check`, {
                    method: 'POST',
                    body: JSON.stringify(payload),
                });
                successCount++;
                setEditingData('provider_keys', index, 'checkStatus', 'success');
            } catch (error) {
                const errorMessage = (error as Error).message || t('unknownError');
                setEditingData('provider_keys', index, 'checkStatus', 'error');
                setEditingData('provider_keys', index, 'checkMessage', errorMessage);
            }
        }
        toastController.info(t('providerEditPage.alert.batchCheckComplete', { success: successCount, total: keysToCheck.length, type: translatedType }));
    };

    const handleConfirmModelSelection = () => {
        const apiKeyIndex = apiKeyIndexToCheck();
        const modelIndex = modelIndexToUse();

        setIsModelSelectModalOpen(false);

        if (modelIndex !== null) {
            if (isBatchCheckingApiKeys()) {
                performBatchApiKeyCheck(modelIndex);
            } else if (apiKeyIndex !== null) {
                performCheck(modelIndex, apiKeyIndex);
            }
        }

        setApiKeyIndexToCheck(null);
        setModelIndexToUse(null);
        setIsBatchCheckingApiKeys(false);
    };

    const handleConfirmApiKeySelection = () => {
        const modelIndex = modelIndexToCheck();
        const apiKeyIndex = apiKeyIndexToUse();

        setIsApiKeySelectModalOpen(false);

        if (apiKeyIndex !== null) {
            if (isBatchCheckingModels()) {
                performBatchModelCheck(apiKeyIndex);
            } else if (modelIndex !== null) {
                performCheck(modelIndex, apiKeyIndex);
            }
        }

        setModelIndexToCheck(null);
        setApiKeyIndexToUse(null);
        setIsBatchCheckingModels(false);
    };

    const handleBatchCheck = async (type: 'models' | 'api_keys') => {
        const currentData = editingData;
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForCheck'));
            return;
        }

        if (type === 'models') {
            const modelsToCheck = currentData.models;
            if (modelsToCheck.length === 0) {
                toastController.info(t('providerEditPage.alert.noModelsToCheck'));
                return;
            }
            const apiKeys = currentData.provider_keys;
            if (apiKeys.length === 0) {
                toastController.warn(t('providerEditPage.alert.noApiKeyForCheck'));
                return;
            }

            if (apiKeys.length === 1) {
                await performBatchModelCheck(0);
            } else {
                setIsBatchCheckingModels(true);
                setApiKeyIndexToUse(0); // Default to first key
                setIsApiKeySelectModalOpen(true);
            }
        } else if (type === 'api_keys') {
            const keysToCheck = currentData.provider_keys;
            if (keysToCheck.length === 0) {
                toastController.info(t('providerEditPage.alert.noApiKeysToCheck'));
                return;
            }
            const models = currentData.models;
            if (models.length === 0) {
                toastController.warn(t('providerEditPage.alert.noModelForCheck'));
                return;
            }

            if (models.length === 1) {
                await performBatchApiKeyCheck(0);
            } else {
                setIsBatchCheckingApiKeys(true);
                setModelIndexToUse(0); // Default to first model
                setIsModelSelectModalOpen(true);
            }
        }
    };

    const handleCheck = async (type: 'model' | 'apiKey', index: number) => {
        const currentData = editingData;
        if (!currentData || !currentData.id) {
            toastController.warn(t('providerEditPage.alert.providerNotSavedForCheck'));
            return;
        }

        if (type === 'model') {
            const apiKeys = currentData.provider_keys;
            if (apiKeys.length === 0) {
                toastController.warn(t('providerEditPage.alert.noApiKeyForCheck'));
                setEditingData('models', index, 'checkStatus', 'error');
                setEditingData('models', index, 'checkMessage', t('providerEditPage.alert.noApiKeyForCheck'));
                return;
            }

            if (apiKeys.length === 1) {
                await performCheck(index, 0);
            } else {
                setModelIndexToCheck(index);
                setApiKeyIndexToUse(0); // Default to first key
                setIsApiKeySelectModalOpen(true);
            }
        } else if (type === 'apiKey') {
            const models = currentData.models;
            if (models.length === 0) {
                toastController.warn(t('providerEditPage.alert.noModelForCheck'));
                setEditingData('provider_keys', index, 'checkStatus', 'error');
                setEditingData('provider_keys', index, 'checkMessage', t('providerEditPage.alert.noModelForCheck'));
                return;
            }

            if (models.length === 1) {
                await performCheck(0, index);
            } else {
                setApiKeyIndexToCheck(index);
                setModelIndexToUse(0); // Default to first model
                setIsModelSelectModalOpen(true);
            }
        }
    };


    const removeListItem = async (field: keyof EditingProviderData, index: number) => {
        const currentData = editingData; // Changed from editingData()
        if (!currentData) return;

        if (field === 'provider_keys') {
            const keyItem = (currentData.provider_keys as LocalProviderApiKeyItem[])[index];
            // Only attempt backend delete if the key has an ID (exists on server)
            // and the provider itself has an ID (is not a new provider).
            if (keyItem.id && currentData.id) {
                try {
                    await request(`/ai/manager/api/provider/${currentData.id}/provider_key/${keyItem.id}`, {
                        method: 'DELETE',
                    });
                    // On successful deletion from backend, remove from local state
                    setEditingData(
                        'provider_keys',
                        produce((keys: LocalProviderApiKeyItem[]) => {
                            keys.splice(index, 1);
                        })
                    );
                    toastController.success(t('providerEditPage.alert.apiKeyDeleteSuccess'));
                } catch (error) {
                    console.error("Failed to delete API key:", error);
                    toastController.error(t('providerEditPage.alert.deleteApiKeyFailed', { error: (error as Error).message || t('unknownError') }));
                    return; // Important: Don't remove from UI if backend deletion failed
                }
            } else {
                // Key not saved in backend or provider not saved, just remove locally
                setEditingData(
                    'provider_keys',
                    produce((keys: LocalProviderApiKeyItem[]) => {
                        keys.splice(index, 1);
                    })
                );
            }
        }
    };

    const updateListItem = <
        F extends keyof EditingProviderData,
        Item extends EditingProviderData[F] extends (infer U)[] ? U : never,
        K extends keyof Item
    >(
        field: F,
        listIndex: number,
        itemField: K,
        value: Item[K]
    ) => {
        setEditingData(field, listIndex, itemField as any, value);
    };

    return (
        <div class="p-4 space-y-6 bg-white rounded-lg shadow-xl max-w-3xl mx-auto my-8">
            <h1 class="text-2xl font-semibold mb-4 text-gray-800">{providerId ? t('providerEditPage.titleEdit') : t('providerEditPage.titleAdd')}</h1>
            <Show when={isLoading()}>
                <div class="text-center py-4 text-gray-500">{t('providerEditPage.loadingData')}</div>
            </Show>
            <Show when={error()}>
                <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                    {error()}
                </div>
            </Show>
            <Show when={!isLoading() && !error() && editingData}>
                <>
                    <div class="space-y-4">
                        <TextField
                            label={<>{t('providerEditPage.labelName')} <span class="text-red-500">*</span></>}
                        value={editingData!.name}
                        onChange={(v) => updateEditingDataField('name', v)}
                    />
                    <TextField
                        label={<>{t('providerEditPage.labelProviderKey')} <span class="text-red-500">*</span></>}
                        value={editingData!.provider_key}
                        onChange={(v) => updateEditingDataField('provider_key', v)}
                        disabled={!!editingData?.id}
                    />
                    <Select
                        label={<>{t('providerEditPage.labelProviderType')} <span class="text-red-500">*</span></>}
                        value={editingData!.provider_type}
                        onChange={(v) => updateEditingDataField('provider_type', v)}
                        options={providerTypes}
                        placeholder={t('providerEditPage.placeholderProviderType')}
                    />
                    <TextField
                        label={<>{t('providerEditPage.labelEndpoint')} <span class="text-red-500">*</span></>}
                        value={editingData!.endpoint}
                        onChange={(v) => updateEditingDataField('endpoint', v)}
                    />
                    <div class="flex items-center space-x-2">
                        <input
                            type="checkbox"
                            id="use_proxy_checkbox"
                            class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                            checked={editingData!.use_proxy}
                            onChange={(e) => updateEditingDataField('use_proxy', e.currentTarget.checked)}
                        />
                        <label for="use_proxy_checkbox" class="text-sm font-medium leading-none">{t('providerEditPage.labelUseProxy')}</label>
                    </div>

                    {/* Update Base Info Button */}
                    <div class="mt-4 mb-6">
                        <Button variant="primary" onClick={handleUpdateProviderBaseInfo}>
                            {editingData?.id ? t('providerEditPage.buttonUpdateBaseInfo') : t('providerEditPage.buttonCreateBaseInfo')}
                        </Button>
                    </div>

                    {/* Models Section */}
                    <div class="section">
                        <div class="flex justify-between items-center mb-2">
                            <h3 class="section-title">{t('providerEditPage.sectionModels')}</h3>
                            <Button variant="secondary" size="sm" onClick={() => handleBatchCheck('models')} disabled={!editingData?.id || editingData.models.length === 0}>
                                {t('providerEditPage.alert.buttonCheckAll')}
                            </Button>
                        </div>
                        <div class="section-header grid grid-cols-[auto_1fr_1fr_auto] gap-2 items-center">
                            <span class="w-5"></span> {/* Status indicator column */}
                            <span class="font-semibold required-field">{t('providerEditPage.tableHeaderModelId')}</span>
                            <span class="font-semibold">{t('providerEditPage.tableHeaderMappedModelId')}</span>
                            <span></span> {/* Placeholder for button column */}
                        </div>
                        <For each={editingData!.models}>
                            {(model, index) => (
                                <div class="section-row grid grid-cols-[auto_1fr_1fr_auto] gap-2 items-center mb-2">
                                    <StatusIndicator status={model.checkStatus} message={model.checkMessage} />
                                    <TextField
                                        value={model.model_name}
                                        onChange={(v) => updateListItem('models', index(), 'model_name', v)}
                                        disabled={!!model.id}
                                        placeholder={t('providerEditPage.placeholderModelId')}
                                    />
                                    <TextField
                                        value={model.real_model_name ?? ''}
                                        onChange={(v) => updateListItem('models', index(), 'real_model_name', v)}
                                        disabled={!!model.id}
                                        placeholder={t('providerEditPage.placeholderMappedModelId')}
                                    />
                                    <div class="flex space-x-1 justify-end md:justify-start">
                                        {/* For new, unsaved models */}
                                        <Show when={!model.id && editingData?.id}>
                                            <Button variant="primary" size="sm" onClick={() => handleSaveSingleModel(index())}>
                                                {t('providerEditPage.buttonSaveModel')}
                                            </Button>
                                            <Button variant="secondary" size="sm" onClick={() => handleCheck('model', index())}>{t('common.check')}</Button>
                                        </Show>
                                        {/* For existing models */}
                                        <Show when={model.id}>
                                            <Button variant="secondary" size="sm" onClick={() => handleCheck('model', index())}>{t('common.check')}</Button>
                                            <Button variant="secondary" size="sm" onClick={() => navigate(`/model/edit/${model.id!}`)}>
                                                {t('common.edit')}
                                            </Button>
                                        </Show>
                                        <Button variant="destructive" size="sm" onClick={() => handleDeleteModel(index())}>{t('common.delete')}</Button>
                                    </div>
                                </div>
                            )}
                        </For>
                        <div class="flex items-center gap-2 mt-2">
                            <Button variant="secondary" size="sm" onClick={() => addListItem<LocalEditableModelItem>('models', { model_name: '', real_model_name: null, isEditing: false })}>{t('providerEditPage.buttonAddModel')}</Button>
                            <Button variant="secondary" size="sm" onClick={handleFetchRemoteModels} disabled={!editingData?.id}>{t('providerEditPage.buttonFetchRemote')}</Button>
                            <Show when={hasUncommittedModels()}>
                                <Button variant="destructive" size="sm" onClick={handleClearUncommittedModels}>{t('providerEditPage.buttonClearUncommitted')}</Button>
                            </Show>
                        </div>
                    </div>

                    {/* API Keys Section */}
                    <div class="section">
                        <div class="flex justify-between items-center mb-2">
                            <h3 class="section-title">{t('providerEditPage.sectionApiKeys')}</h3>
                            <Button variant="secondary" size="sm" onClick={() => handleBatchCheck('api_keys')} disabled={!editingData?.id || editingData.provider_keys.length === 0}>
                                {t('providerEditPage.alert.buttonCheckAll')}
                            </Button>
                        </div>
                        {/* Adjusted grid to make space for the new save button */}
                        <div class="section-header grid grid-cols-[auto_2fr_1fr_auto] gap-2 items-center">
                            <span class="w-5"></span> {/* Status indicator column */}
                            <span class="font-semibold required-field">{t('providerEditPage.tableHeaderApiKey')}</span>
                            <span class="font-semibold">{t('providerEditPage.tableHeaderDescription')}</span>
                            <span class="text-center">{t('providerEditPage.tableHeaderActions')}</span>
                        </div>
                        <For each={editingData!.provider_keys}>
                            {(keyItem, index) => (
                                <div class="section-row grid grid-cols-[auto_2fr_1fr_auto] gap-2 items-center mb-2">
                                    <StatusIndicator status={keyItem.checkStatus} message={keyItem.checkMessage} />
                                    <TextField
                                        value={keyItem.api_key}
                                        onChange={(v) => updateListItem<LocalProviderApiKeyItem>('provider_keys', index(), 'api_key', v)}
                                        disabled={!!keyItem.id} // API Key string is not editable for existing keys
                                        placeholder={t('providerEditPage.placeholderApiKey')}
                                        type={editingData?.provider_type === 'VERTEX' || !!keyItem.id ? 'text' : 'password'}
                                    />
                                    <TextField
                                        value={keyItem.description ?? ''}
                                        onChange={(v) => updateListItem<LocalProviderApiKeyItem>('provider_keys', index(), 'description', v)}
                                        disabled={!!keyItem.id && !keyItem.isEditing} // Description editable for new keys or existing keys in edit mode
                                        placeholder={t('providerEditPage.placeholderDescription')}
                                    />
                                    <div class="flex space-x-1 justify-end md:justify-start">
                                        {/* For new, unsaved keys */}
                                        <Show when={!keyItem.id && editingData?.id}>
                                            <Button variant="primary" size="sm" onClick={() => handleSaveSingleApiKey(index())}>
                                                {t('providerEditPage.buttonSaveThisKey')}
                                            </Button>
                                            <Button variant="secondary" size="sm" onClick={() => handleCheck('apiKey', index())}>{t('common.check')}</Button>
                                        </Show>
                                        {/* For existing keys, not in edit mode */}
                                        <Show when={keyItem.id && !keyItem.isEditing}>
                                            <Button variant="secondary" size="sm" onClick={() => handleCheck('apiKey', index())}>{t('common.check')}</Button>
                                            <Button variant="secondary" size="sm" onClick={() => handleEditApiKey(index())}>
                                                {t('common.edit')}
                                            </Button>
                                        </Show>
                                        {/* For existing keys, in edit mode */}
                                        <Show when={keyItem.id && keyItem.isEditing}>
                                            <Button variant="primary" size="sm" onClick={() => handleSaveSingleApiKey(index())}>
                                                {t('common.save')}
                                            </Button>
                                            <Button variant="secondary" size="sm" onClick={() => handleCancelEditApiKey(index())}>
                                                {t('common.cancel')}
                                            </Button>
                                        </Show>
                                        <Button variant="destructive" size="sm" onClick={() => removeListItem('provider_keys', index())}>{t('common.delete')}</Button>
                                    </div>
                                </div>
                            )}
                        </For>
                        <Button variant="secondary" size="sm" class="mt-2" onClick={() => addListItem<LocalProviderApiKeyItem>('provider_keys', { api_key: '', description: null, isEditing: false })}>{t('providerEditPage.buttonAddApiKey')}</Button>
                    </div>

                    {/* Custom Fields Section */}
                    <div class="section">
                        <h3 class="section-title">{t('providerEditPage.sectionCustomFields')}</h3>
                        <div class="section-header grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-2 items-center">
                            <span class="font-semibold">{t('providerEditPage.tableHeaderFieldName')}</span>
                            <span class="font-semibold">{t('providerEditPage.tableHeaderFieldValue')}</span>
                            <span class="font-semibold">{t('providerEditPage.tableHeaderDescription')}</span>
                            <span class="font-semibold">{t('providerEditPage.tableHeaderFieldType')}</span>
                            <span></span>
                        </div>
                        <For each={editingData!.custom_fields}>
                            {(field, index) => (
                                <div class="section-row grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-2 items-center mb-2">
                                    <TextField value={field.field_name} disabled />
                                    <TextField value={field.field_value} disabled />
                                    <TextField value={field.description ?? ''} disabled />
                                    <TextField value={field.field_type} disabled />
                                    <Button variant="destructive" size="sm" onClick={() => handleUnlinkCustomField(field.id, index())}>{t('common.delete')}</Button>
                                </div>
                            )}
                        </For>
                        <div class="mt-4 flex items-center gap-2">
                            <Select
                                value={availableCustomFields().find(f => f.id === selectedCustomFieldId())}
                                onChange={(v) => setSelectedCustomFieldId(v ? v.id : null)}
                                options={availableCustomFields()}
                                optionValue="id"
                                optionTextValue="field_name"
                                placeholder={t('providerEditPage.placeholderSelectCustomField')}
                            />
                            <Button variant="primary" size="sm" onClick={handleLinkCustomField} disabled={!selectedCustomFieldId()}>
                                {t('providerEditPage.buttonAddCustomField')}
                            </Button>
                        </div>
                    </div>
                    {/* Removed main Save/Cancel buttons, keeping only a top-level navigate back button if desired */}
                    {/* For now, assuming the user wants all bottom action buttons removed. */}
                    {/* If a general "Back" or "Done" button is needed, it can be added here, calling handleNavigateBack */}
                    <div class="mt-6 flex justify-end space-x-2 pt-4 border-t">
                        <Button variant="secondary" onClick={handleNavigateBack}>{t('providerEditPage.buttonBackToList')}</Button>
                    </div>
                </div>
                <Show when={isModelSelectModalOpen()}>
                    <div class="fixed inset-0 bg-black bg-opacity-50 flex justify-center items-center z-50">
                        <div class="bg-white p-6 rounded-lg shadow-xl w-full max-w-md">
                            <h2 class="text-lg font-bold mb-4">{t('providerEditPage.modalSelectModel.title')}</h2>
                            <p class="mb-4">{t('providerEditPage.modalSelectModel.description')}</p>
                            <Select
                                value={modelOptionsForSelect().find(opt => opt.value === modelIndexToUse())}
                                onChange={(v) => setModelIndexToUse(v ? v.value : null)}
                                options={modelOptionsForSelect()}
                                optionValue="value"
                                optionTextValue="label"
                                placeholder={t('providerEditPage.modalSelectModel.selectPlaceholder')}
                            />
                            <div class="mt-6 flex justify-end space-x-2">
                                <Button variant="secondary" onClick={() => setIsModelSelectModalOpen(false)}>{t('common.cancel')}</Button>
                                <Button variant="primary" onClick={handleConfirmModelSelection} disabled={modelIndexToUse() === null}>{t('common.check')}</Button>
                            </div>
                        </div>
                    </div>
                </Show>
                <Show when={isApiKeySelectModalOpen()}>
                    <div class="fixed inset-0 bg-black bg-opacity-50 flex justify-center items-center z-50">
                        <div class="bg-white p-6 rounded-lg shadow-xl w-full max-w-md">
                            <h2 class="text-lg font-bold mb-4">{t('providerEditPage.modalSelectApiKey.title')}</h2>
                            <p class="mb-4">{t('providerEditPage.modalSelectApiKey.description')}</p>
                            <Select
                                value={apiKeyOptionsForSelect().find(opt => opt.value === apiKeyIndexToUse())}
                                onChange={(v) => setApiKeyIndexToUse(v ? v.value : null)}
                                options={apiKeyOptionsForSelect()}
                                optionValue="value"
                                optionTextValue="label"
                                placeholder={t('providerEditPage.modalSelectApiKey.selectPlaceholder')}
                            />
                            <div class="mt-6 flex justify-end space-x-2">
                                <Button variant="secondary" onClick={() => setIsApiKeySelectModalOpen(false)}>{t('common.cancel')}</Button>
                                <Button variant="primary" onClick={handleConfirmApiKeySelection} disabled={apiKeyIndexToUse() === null}>{t('common.check')}</Button>
                            </div>
                        </div>
                    </div>
                </Show>
                </>
            </Show>
        </div>
    );
}
