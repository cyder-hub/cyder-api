import { createSignal, For, Show, createResource } from 'solid-js';
import { useI18n } from '../i18n';
import { Button } from '@kobalte/core/button';
import { TextField } from '@kobalte/core/text-field';
import { Select } from '@kobalte/core/select';
import { request } from '../services/api';

// --- Type Definitions ---

type CustomFieldType = 'STRING' | 'INTEGER' | 'NUMBER' | 'BOOLEAN' | 'JSON_STRING' | 'UNSET';
const fieldPlacements = ['HEADER', 'QUERY', 'BODY'];

interface CustomFieldDefinition {
    id: number;
    name: string | null;
    description: string | null;
    field_name: string;
    field_placement: string;
    field_type: string;
    string_value: string | null;
    integer_value: number | null;
    number_value: number | null;
    boolean_value: boolean | null;
    is_enabled: boolean;
}

interface EditingCustomField {
    id: number | null;
    name: string | null;
    description: string | null;
    field_name: string;
    field_placement: string;
    field_type: CustomFieldType;
    string_value: string | null;
    integer_value: number | null;
    number_value: number | null;
    boolean_value: boolean | null;
    is_enabled: boolean;
}

const newCustomFieldTemplate = (): EditingCustomField => ({
    id: null,
    name: '',
    description: '',
    field_name: '',
    field_placement: '',
    field_type: 'UNSET',
    string_value: null,
    integer_value: null,
    number_value: null,
    boolean_value: null,
    is_enabled: true,
});

const fieldTypes: CustomFieldType[] = ['STRING', 'INTEGER', 'NUMBER', 'BOOLEAN', 'JSON_STRING'];

// --- API Functions ---

const fetchCustomFieldsAPI = async (): Promise<CustomFieldDefinition[]> => {
    try {
        const response = await request("/ai/manager/api/custom_field_definition/list?page_size=1000");
        return response.list || [];
    } catch (error) {
        console.error("Failed to fetch custom fields:", error);
        return [];
    }
};

const fetchCustomFieldDetailAPI = async (id: number): Promise<CustomFieldDefinition | null> => {
    try {
        const response = await request(`/ai/manager/api/custom_field_definition/${id}`);
        return response as CustomFieldDefinition;
    } catch (error) {
        console.error(`Failed to fetch custom field detail for id ${id}:`, error);
        return null;
    }
};

const saveCustomFieldAPI = async (field: EditingCustomField): Promise<any> => {
    const payload = {
        name: field.name,
        description: field.description,
        field_name: field.field_name,
        field_placement: field.field_placement,
        field_type: field.field_type,
        string_value: field.field_type === 'STRING' || field.field_type === 'JSON_STRING' ? field.string_value : null,
        integer_value: field.field_type === 'INTEGER' ? field.integer_value : null,
        number_value: field.field_type === 'NUMBER' ? field.number_value : null,
        boolean_value: field.field_type === 'BOOLEAN' ? field.boolean_value : null,
        is_enabled: field.is_enabled,
    };

    const url = field.id
        ? `/ai/manager/api/custom_field_definition/${field.id}`
        : '/ai/manager/api/custom_field_definition';
    const method = field.id ? 'PUT' : 'POST';

    return request(url, {
        method: method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
    });
};

const deleteCustomFieldAPI = async (id: number): Promise<any> => {
    return request(`/ai/manager/api/custom_field_definition/${id}`, { method: 'DELETE' });
};

// --- Component ---
export default function CustomFieldsPage() {
    const [t] = useI18n();
    const [customFields, { refetch: refetchCustomFields }] = createResource<CustomFieldDefinition[]>(fetchCustomFieldsAPI, { initialValue: [] });
    const [showEditModal, setShowEditModal] = createSignal(false);
    const [editingField, setEditingField] = createSignal<EditingCustomField>(newCustomFieldTemplate());

    const handleOpenAddModal = () => {
        setEditingField(newCustomFieldTemplate());
        setShowEditModal(true);
    };

    const handleOpenEditModal = async (field: CustomFieldDefinition) => {
        const detail = await fetchCustomFieldDetailAPI(field.id);
        if (detail) {
            setEditingField({
                id: detail.id,
                name: detail.name,
                description: detail.description,
                field_name: detail.field_name,
                field_placement: detail.field_placement,
                field_type: detail.field_type as CustomFieldType,
                string_value: detail.string_value,
                integer_value: detail.integer_value,
                number_value: detail.number_value,
                boolean_value: detail.boolean_value,
                is_enabled: detail.is_enabled,
            });
            setShowEditModal(true);
        } else {
            alert(t('customFieldsPage.alert.loadDetailFailed'));
        }
    };

    const handleCloseModal = () => {
        setShowEditModal(false);
    };

    const handleSave = async () => {
        const field = editingField();
        if (!field) return;
        if (!field.field_name.trim() || field.field_type === 'UNSET') {
            alert(t('customFieldsPage.alert.nameAndTypeRequired'));
            return;
        }
        try {
            await saveCustomFieldAPI(field);
            setShowEditModal(false);
            refetchCustomFields();
        } catch (error) {
            console.error("Failed to save custom field:", error);
            alert(t('customFieldsPage.alert.saveFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
        }
    };

    const handleToggleEnable = async (field: CustomFieldDefinition) => {
        const updatedField: EditingCustomField = {
            ...field,
            field_type: field.field_type as CustomFieldType,
            is_enabled: !field.is_enabled,
        };
        try {
            await saveCustomFieldAPI(updatedField);
            refetchCustomFields();
        } catch (error) {
            console.error("Failed to toggle custom field status:", error);
            alert(t('customFieldsPage.alert.toggleFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
        }
    };

    const handleDelete = async (id: number, name: string) => {
        if (confirm(t('customFieldsPage.confirmDelete', { name: name }))) {
            try {
                await deleteCustomFieldAPI(id);
                refetchCustomFields();
            } catch (error) {
                console.error("Failed to delete custom field:", error);
                alert(t('customFieldsPage.alert.deleteFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
            }
        }
    };

    return (
        <div class="p-4">
            <div class="flex justify-between items-center mb-6">
                <h1 class="text-2xl font-semibold text-gray-800">{t('customFieldsPage.title')}</h1>
                <Button onClick={handleOpenAddModal} class="btn btn-primary">{t('customFieldsPage.addCustomField')}</Button>
            </div>

            {/* Data Table */}
            <Show when={customFields.loading}>
                <div class="text-center py-4 text-gray-500">{t('loading')}</div>
            </Show>
            <Show when={!customFields.loading && customFields.error}>
                <div class="text-center py-4 text-red-500">{t('customFieldsPage.errorPrefix')}</div>
            </Show>
            <Show when={!customFields.loading && !customFields.error && customFields()?.length === 0}>
                <div class="text-center py-4 text-gray-500">{t('customFieldsPage.noData')}</div>
            </Show>
            <Show when={!customFields.loading && !customFields.error && customFields() && customFields()!.length > 0}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <table class="min-w-full divide-y divide-gray-200 data-table">
                        <thead class="bg-gray-100">
                            <tr>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('customFieldsPage.table.name')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('customFieldsPage.table.fieldName')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('customFieldsPage.table.fieldType')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('customFieldsPage.table.placement')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('customFieldsPage.table.enabled')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('actions')}</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">
                            <For each={customFields()}>{(field) =>
                                <tr class="hover:bg-gray-50">
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800">{field.name}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{field.field_name}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{field.field_type}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{field.field_placement}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">
                                        <input
                                            type="checkbox"
                                            class="form-checkbox"
                                            checked={field.is_enabled}
                                            onChange={() => handleToggleEnable(field)}
                                        />
                                    </td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm space-x-2">
                                        <Button onClick={() => handleOpenEditModal(field)} class="btn btn-primary btn-sm">{t('edit')}</Button>
                                        <Button onClick={() => handleDelete(field.id, field.name || field.field_name)} class="btn btn-danger btn-sm">{t('delete')}</Button>
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
                    <div class="bg-white rounded-lg shadow-xl p-6 space-y-4 w-full max-w-lg model" style="height: auto;">
                        <h2 class="text-xl font-semibold text-gray-800 model-title mb-6">
                            {editingField()?.id ? t('customFieldsPage.modal.titleEdit') : t('customFieldsPage.modal.titleAdd')}
                        </h2>

                        <TextField class="form-item" value={editingField()?.name || ''} onChange={(v) => setEditingField(s => ({ ...s!, name: v }))}>
                            <TextField.Label class="form-label">{t('customFieldsPage.modal.labelName')}</TextField.Label>
                            <TextField.Input class="form-input" placeholder={t('customFieldsPage.modal.placeholderName')} />
                        </TextField>

                        <TextField class="form-item" value={editingField()?.description || ''} onChange={(v) => setEditingField(s => ({ ...s!, description: v }))}>
                            <TextField.Label class="form-label">{t('customFieldsPage.modal.labelDescription')}</TextField.Label>
                            <TextField.Input class="form-input" placeholder={t('customFieldsPage.modal.placeholderDescription')} />
                        </TextField>

                        <TextField class="form-item" value={editingField()?.field_name || ''} onChange={(v) => setEditingField(s => ({ ...s!, field_name: v }))}>
                            <TextField.Label class="form-label">{t('customFieldsPage.modal.labelFieldName')}</TextField.Label>
                            <TextField.Input class="form-input" placeholder={t('customFieldsPage.modal.placeholderFieldName')} />
                        </TextField>

                        <div class="form-item">
                            <Select
                                value={editingField()?.field_placement}
                                onChange={(v) => setEditingField(s => ({ ...s!, field_placement: v || '' }))}
                                options={fieldPlacements}
                                placeholder={t('customFieldsPage.modal.placeholderPlacement')}
                                itemComponent={props => (
                                    <Select.Item item={props.item} class="select__item p-2 hover:bg-gray-100 cursor-pointer">
                                        <Select.ItemLabel>{props.item.rawValue}</Select.ItemLabel>
                                    </Select.Item>
                                )}
                            >
                                <Select.Label class="form-label">{t('customFieldsPage.modal.labelPlacement')}</Select.Label>
                                <Select.Trigger class="form-input w-full flex justify-between items-center" aria-label="Placement">
                                    <Select.Value>
                                        {(state) => state.selectedOption() || <span class="text-gray-500">{t('customFieldsPage.modal.placeholderPlacement')}</span>}
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

                        <div class="form-item">
                            <Select<CustomFieldType>
                                value={editingField()?.field_type}
                                onChange={(v) => setEditingField(s => ({ ...s!, field_type: v || 'UNSET' }))}
                                options={fieldTypes}
                                placeholder={t('customFieldsPage.modal.placeholderFieldType')}
                                itemComponent={props => (
                                    <Select.Item item={props.item} class="select__item p-2 hover:bg-gray-100 cursor-pointer">
                                        <Select.ItemLabel>{props.item.rawValue}</Select.ItemLabel>
                                    </Select.Item>
                                )}
                            >
                                <Select.Label class="form-label">{t('customFieldsPage.modal.labelFieldType')}</Select.Label>
                                <Select.Trigger class="form-input w-full flex justify-between items-center" aria-label="Field Type">
                                    <Select.Value<CustomFieldType>>
                                        {(state) => state.selectedOption() || <span class="text-gray-500">{t('customFieldsPage.modal.placeholderFieldType')}</span>}
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

                        <Show when={editingField()?.field_type === 'STRING'}>
                            <TextField class="form-item" value={editingField()?.string_value || ''} onChange={(v) => setEditingField(s => ({ ...s!, string_value: v }))}>
                                <TextField.Label class="form-label">{t('customFieldsPage.modal.labelValue')}</TextField.Label>
                                <TextField.Input class="form-input" placeholder={t('customFieldsPage.modal.placeholderStringValue')} />
                            </TextField>
                        </Show>
                        <Show when={editingField()?.field_type === 'JSON_STRING'}>
                            <TextField class="form-item" value={editingField()?.string_value || ''} onChange={(v) => setEditingField(s => ({ ...s!, string_value: v }))}>
                                <TextField.Label class="form-label">{t('customFieldsPage.modal.labelValue')}</TextField.Label>
                                <TextField.Input class="form-input" placeholder={t('customFieldsPage.modal.placeholderJsonStringValue')} />
                            </TextField>
                        </Show>
                        <Show when={editingField()?.field_type === 'INTEGER'}>
                            <TextField class="form-item" value={editingField()?.integer_value?.toString() || ''} onChange={(v) => setEditingField(s => ({ ...s!, integer_value: parseInt(v, 10) || null }))}>
                                <TextField.Label class="form-label">{t('customFieldsPage.modal.labelValue')}</TextField.Label>
                                <TextField.Input type="number" class="form-input" placeholder={t('customFieldsPage.modal.placeholderIntegerValue')} />
                            </TextField>
                        </Show>
                        <Show when={editingField()?.field_type === 'NUMBER'}>
                            <TextField class="form-item" value={editingField()?.number_value?.toString() || ''} onChange={(v) => setEditingField(s => ({ ...s!, number_value: parseFloat(v) || null }))}>
                                <TextField.Label class="form-label">{t('customFieldsPage.modal.labelValue')}</TextField.Label>
                                <TextField.Input type="number" step="any" class="form-input" placeholder={t('customFieldsPage.modal.placeholderNumberValue')} />
                            </TextField>
                        </Show>
                        <Show when={editingField()?.field_type === 'BOOLEAN'}>
                            <div class="form-item flex items-center">
                                <label for="boolean_value_checkbox" class="form-label mr-4">{t('customFieldsPage.modal.labelValue')}</label>
                                <input
                                    type="checkbox"
                                    id="boolean_value_checkbox"
                                    class="form-checkbox"
                                    checked={editingField()?.boolean_value || false}
                                    onChange={(e) => setEditingField(s => ({ ...s!, boolean_value: e.currentTarget.checked }))}
                                />
                            </div>
                        </Show>

                        <div class="form-item flex items-center">
                            <label for="is_enabled_checkbox" class="form-label mr-4">{t('customFieldsPage.modal.labelEnabled')}</label>
                            <input
                                type="checkbox"
                                id="is_enabled_checkbox"
                                class="form-checkbox"
                                checked={editingField()?.is_enabled || false}
                                onChange={(e) => setEditingField(s => ({ ...s!, is_enabled: e.currentTarget.checked }))}
                            />
                        </div>

                        <div class="form-buttons flex justify-end gap-3 pt-4">
                            <Button onClick={handleCloseModal} class="btn btn-default">{t('common.cancel')}</Button>
                            <Button onClick={handleSave} class="btn btn-primary">{t('common.save')}</Button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    );
}
