import { createSignal, Show, onMount, Accessor, Setter, createEffect } from 'solid-js';
import { useI18n } from '../i18n'; // Import the i18n hook
import { Button } from '@kobalte/core/button';
import { TextField } from '@kobalte/core/text-field';
import { Checkbox } from '@kobalte/core/checkbox';
import { Dialog } from '@kobalte/core/dialog';
import { request } from '../services/api';
import type { ApiKeyItem } from '../store/types';

export interface EditingApiKeyData {
    id: number | null; // null for new
    name: string;
    api_key: string;
    description: string;
    is_enabled: boolean;
}

interface ApiKeyEditModalProps {
    isOpen: Accessor<boolean>;
    onClose: () => void;
    initialData: Accessor<ApiKeyItem | null>; // Pass the full ApiKeyItem or null for new
    onSaveSuccess: () => void;
}

const getEmptyEditingData = (): EditingApiKeyData => ({
    id: null,
    name: '',
    api_key: '',
    description: '',
    is_enabled: true,
});

export default function ApiKeyEditModal(props: ApiKeyEditModalProps) {
    const [ t ] = useI18n(); // Initialize the t function
    const [editingData, setEditingData] = createSignal<EditingApiKeyData>(getEmptyEditingData());
    const [showApiKeyInForm, setShowApiKeyInForm] = createSignal(false);

    // Effect to update form state when the modal is opened or initialData changes.
    // This runs when props.isOpen() or props.initialData() changes.
    createEffect(() => {
        if (props.isOpen()) {
            const currentInitial = props.initialData();
            if (currentInitial) {
                // Modal is open for editing an existing item
                setEditingData({
                    id: currentInitial.id,
                    name: currentInitial.name,
                    api_key: '', // API key is not pre-filled for editing for security
                    description: currentInitial.description,
                    is_enabled: currentInitial.is_enabled,
                });
            } else {
                // Modal is open for creating a new item
                setEditingData(getEmptyEditingData());
            }
            setShowApiKeyInForm(false); // Reset API key visibility each time the modal content is set
        }
        // No 'else' block is strictly necessary here to reset editingData when props.isOpen() is false,
        // because the form is not visible, and the parent component (ApiKeyPage)
        // already clears its `selectedApiKey` state when the modal is closed,
        // which means `props.initialData()` will be null for the next "new" item.
    });


    const handleCommit = async () => {
        const currentFormState = editingData();

        if (!currentFormState) {
            alert(t('apiKeyEditModal.alert.formDataError'));
            return;
        }

        if (!currentFormState.name.trim()) {
            alert(t('apiKeyEditModal.alert.nameRequired'));
            return;
        }
        if (currentFormState.id === null && !currentFormState.api_key.trim()) {
            alert(t('apiKeyEditModal.alert.apiKeyRequired'));
            return;
        }

        const payload: any = {
            name: currentFormState.name,
            description: currentFormState.description,
            is_enabled: currentFormState.is_enabled,
        };

        if (currentFormState.api_key.trim()) {
            payload.api_key = currentFormState.api_key;
        }

        const method = currentFormState.id ? 'PUT' : 'POST';
        const url = currentFormState.id ? `/ai/manager/system_api/api_key/${currentFormState.id}` : '/ai/manager/api/system_api_key';

        try {
            await request(url, {
                method: method,
                body: JSON.stringify(payload)
            });
            props.onSaveSuccess(); // This will trigger refetch and close in parent
            props.onClose();
        } catch (error) {
            console.error("Failed to commit API key:", error);
            alert(t('apiKeyEditModal.alert.saveFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const toggleApiKeyVisibility = () => {
        setShowApiKeyInForm(!showApiKeyInForm());
    };

    return (
        <Show when={props.isOpen()}>
            <Dialog open={props.isOpen()} onOpenChange={(isOpen) => !isOpen && props.onClose()} modal>
                <Dialog.Portal>
                    <Dialog.Overlay class="fixed inset-0 bg-black bg-opacity-50" />
                    <div class="fixed inset-0 flex items-center justify-center p-4">
                        <Dialog.Content class="bg-white p-6 rounded-lg shadow-xl w-full max-w-lg max-h-[90vh] flex flex-col model">
                            <Dialog.Title class="text-xl font-semibold mb-4 model-title">{editingData()?.id ? t('apiKeyEditModal.titleEdit') : t('apiKeyEditModal.titleAdd')}</Dialog.Title>
                            <div class="overflow-y-auto space-y-4 pr-2">
                                <TextField class="form-item" value={editingData()?.name ?? ''} onChange={(v) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), name: v }))}>
                                    <TextField.Label class="form-label">{t('apiKeyEditModal.labelName')} <span class="text-red-500">*</span></TextField.Label>
                                    <TextField.Input class="form-input" />
                                </TextField>

                                <TextField class="form-item">
                                    <TextField.Label class="form-label">
                                        {t('apiKeyEditModal.labelApiKey')}
                                        <Show when={editingData()?.id === null}><span class="text-red-500">*</span></Show>
                                        <Show when={editingData()?.id !== null}><span class="text-gray-500 text-xs ml-1">{t('apiKeyEditModal.apiKeyHelpText')}</span></Show>
                                    </TextField.Label>
                                    <div class="flex items-center space-x-2">
                                        <TextField.Input
                                            class="form-input flex-grow"
                                            type={showApiKeyInForm() ? 'text' : 'password'}
                                            value={editingData()?.api_key ?? ''}
                                            onInput={(e) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), api_key: e.currentTarget.value }))}
                                            placeholder={editingData()?.id ? t('apiKeyEditModal.apiKeyPlaceholderEdit') : t('apiKeyEditModal.apiKeyPlaceholderNew')}
                                        />
                                        <Button class="btn btn-secondary btn-sm" onClick={toggleApiKeyVisibility}>
                                            {showApiKeyInForm() ? t('apiKeyEditModal.buttonHide') : t('apiKeyEditModal.buttonShow')}
                                        </Button>
                                    </div>
                                </TextField>

                                <TextField class="form-item" value={editingData()?.description ?? ''} onChange={(v) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), description: v }))}>
                                    <TextField.Label class="form-label">{t('apiKeyEditModal.labelDescription')}</TextField.Label>
                                    <TextField.Input class="form-input" />
                                </TextField>

                                <Checkbox class="form-item items-center" checked={editingData()?.is_enabled ?? false} onChange={(v) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), is_enabled: v }))}>
                                    <Checkbox.Input class="form-checkbox" />
                                    <Checkbox.Label class="form-label ml-2">{t('apiKeyEditModal.labelEnabled')}</Checkbox.Label>
                                </Checkbox>
                            </div>
                            <Dialog.CloseButton class="absolute top-4 right-4 p-1 rounded-full hover:bg-gray-200 transition-colors" onClick={props.onClose}>
                                <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-gray-600" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
                            </Dialog.CloseButton>
                            <div class="mt-6 flex justify-end space-x-2 pt-4 border-t">
                                <Button class="btn btn-secondary" onClick={props.onClose}>{t('common.cancel')}</Button>
                                <Button class="btn btn-primary" onClick={handleCommit}>{t('common.save')}</Button>
                            </div>
                        </Dialog.Content>
                    </div>
                </Dialog.Portal>
            </Dialog>
        </Show>
    );
}
