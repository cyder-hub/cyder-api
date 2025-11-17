import { createSignal, Show, onMount, Accessor, Setter, createEffect, For, createMemo } from 'solid-js';
import { useI18n } from '../i18n'; // Import the i18n hook
import { Button } from './ui/Button';
import { TextField } from './ui/Input';
import { Select } from './ui/Select';
import { DialogRoot, DialogContent, DialogHeader, DialogFooter, DialogTitle } from './ui/Dialog';
import { request } from '../services/api';
import type { ApiKeyItem } from '../store/types';
import { policies, type AccessControlPolicyFromAPI } from '../store/accessControlStore';

export interface EditingApiKeyData {
    id: number | null; // null for new
    name: string;
    api_key: string;
    description: string;
    is_enabled: boolean;
    access_control_policy_id: number | null;
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
    access_control_policy_id: null,
});

export default function ApiKeyEditModal(props: ApiKeyEditModalProps) {
    const [ t ] = useI18n(); // Initialize the t function
    const [editingData, setEditingData] = createSignal<EditingApiKeyData>(getEmptyEditingData());

    const policyOptions = createMemo(() => {
        const noPolicy = { value: null, label: t('apiKeyEditModal.noPolicy') };
        const policiesList = (policies() || []).map(p => ({ value: p.id, label: p.name }));
        return [noPolicy, ...policiesList];
    });

    const selectedPolicy = createMemo(() => {
        return policyOptions().find(p => p.value === editingData()?.access_control_policy_id);
    });

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
                    access_control_policy_id: (currentInitial as any).access_control_policy_id ?? null,
                });
            } else {
                // Modal is open for creating a new item
                setEditingData(getEmptyEditingData());
            }
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

        const payload: any = {
            name: currentFormState.name,
            description: currentFormState.description,
            is_enabled: currentFormState.is_enabled,
            access_control_policy_id: currentFormState.access_control_policy_id,
        };

        const method = currentFormState.id ? 'PUT' : 'POST';
        const url = currentFormState.id ? `/ai/manager/api/system_api_key/${currentFormState.id}` : '/ai/manager/api/system_api_key';

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

    return (
        <DialogRoot open={props.isOpen()} onOpenChange={(isOpen) => !isOpen && props.onClose()} modal>
            <DialogContent class="max-h-[90vh] flex flex-col">
                <DialogHeader>
                    <DialogTitle>{editingData()?.id ? t('apiKeyEditModal.titleEdit') : t('apiKeyEditModal.titleAdd')}</DialogTitle>
                </DialogHeader>
                <div class="overflow-y-auto space-y-4 pr-2">
                        <TextField
                            label={<>{t('apiKeyEditModal.labelName')} <span class="text-red-500">*</span></>}
                            value={editingData()?.name ?? ''}
                            onChange={(v) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), name: v }))}
                        />

                        <TextField
                            label={t('apiKeyEditModal.labelDescription')}
                            value={editingData()?.description ?? ''}
                            onChange={(v) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), description: v }))}
                        />

                        <Select
                            label={t('apiKeyEditModal.labelAccessControlPolicy')}
                            value={selectedPolicy()}
                            onChange={(v) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), access_control_policy_id: v ? v.value : null }))}
                            options={policyOptions()}
                            optionValue="value"
                            optionTextValue="label"
                            placeholder={t('apiKeyEditModal.placeholderAccessControlPolicy')}
                        />

                        <div class="flex items-center space-x-2">
                            <input
                                type="checkbox"
                                id="is_enabled_modal_checkbox"
                                class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                                checked={editingData()?.is_enabled ?? false}
                                onChange={(e) => setEditingData(prev => ({ ...(prev ?? getEmptyEditingData()), is_enabled: e.currentTarget.checked }))}
                            />
                            <label for="is_enabled_modal_checkbox" class="text-sm font-medium leading-none">{t('apiKeyEditModal.labelEnabled')}</label>
                        </div>
                    </div>
                    <DialogFooter class="mt-6 pt-4 border-t">
                        <Button variant="secondary" onClick={props.onClose}>{t('common.cancel')}</Button>
                        <Button variant="primary" onClick={handleCommit}>{t('common.save')}</Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>
    );
}
