import { createSignal, For, Show, createResource } from 'solid-js';
import { useI18n } from '../i18n';
import { Button } from '../components/ui/Button';
import { request } from '../services/api';
import {
    DialogRoot,
    DialogContent,
    DialogHeader,
    DialogFooter,
    DialogTitle,
} from '../components/ui/Dialog';
import { TextField, NumberField } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '../components/ui/Table';

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
                <Button onClick={handleOpenAddModal} variant="primary">{t('customFieldsPage.addCustomField')}</Button>
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
                <div class="shadow-md rounded-lg border border-gray-200 overflow-hidden">
                    <TableRoot>
                        <TableHeader>
                            <TableRow>
                                <TableColumnHeader>{t('customFieldsPage.table.name')}</TableColumnHeader>
                                <TableColumnHeader>{t('customFieldsPage.table.fieldName')}</TableColumnHeader>
                                <TableColumnHeader>{t('customFieldsPage.table.fieldType')}</TableColumnHeader>
                                <TableColumnHeader>{t('customFieldsPage.table.placement')}</TableColumnHeader>
                                <TableColumnHeader>{t('customFieldsPage.table.enabled')}</TableColumnHeader>
                                <TableColumnHeader>{t('actions')}</TableColumnHeader>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            <For each={customFields()}>{(field) =>
                                <TableRow>
                                    <TableCell>{field.name}</TableCell>
                                    <TableCell>{field.field_name}</TableCell>
                                    <TableCell>{field.field_type}</TableCell>
                                    <TableCell>{field.field_placement}</TableCell>
                                    <TableCell>
                                        <input
                                            type="checkbox"
                                            class="form-checkbox"
                                            checked={field.is_enabled}
                                            onChange={() => handleToggleEnable(field)}
                                        />
                                    </TableCell>
                                    <TableCell>
                                        <Button onClick={() => handleOpenEditModal(field)} variant="secondary" size="sm">{t('edit')}</Button>
                                        <Button onClick={() => handleDelete(field.id, field.name || field.field_name)} variant="destructive" size="sm">{t('delete')}</Button>
                                    </TableCell>
                                </TableRow>
                            }</For>
                        </TableBody>
                    </TableRoot>
                </div>
            </Show>

            {/* Edit/Add Modal */}
            <DialogRoot open={showEditModal()} onOpenChange={setShowEditModal}>
                <DialogContent class="space-y-4">
                    <DialogHeader>
                        <DialogTitle>
                            {editingField()?.id ? t('customFieldsPage.modal.titleEdit') : t('customFieldsPage.modal.titleAdd')}
                        </DialogTitle>
                    </DialogHeader>

                    <TextField
                        label={t('customFieldsPage.modal.labelName')}
                        placeholder={t('customFieldsPage.modal.placeholderName')}
                        value={editingField()?.name || ''}
                        onChange={(v) => setEditingField(s => ({ ...s!, name: v }))}
                    />

                    <TextField
                        label={t('customFieldsPage.modal.labelDescription')}
                        placeholder={t('customFieldsPage.modal.placeholderDescription')}
                        value={editingField()?.description || ''}
                        onChange={(v) => setEditingField(s => ({ ...s!, description: v }))}
                    />

                    <TextField
                        label={t('customFieldsPage.modal.labelFieldName')}
                        placeholder={t('customFieldsPage.modal.placeholderFieldName')}
                        value={editingField()?.field_name || ''}
                        onChange={(v) => setEditingField(s => ({ ...s!, field_name: v }))}
                    />

                    <Select
                        value={editingField()?.field_placement}
                        onChange={(v) => setEditingField(s => ({ ...s!, field_placement: v || '' }))}
                        options={fieldPlacements}
                        label={t('customFieldsPage.modal.labelPlacement')}
                        placeholder={t('customFieldsPage.modal.placeholderPlacement')}
                    />

                    <Select<CustomFieldType>
                        value={editingField()?.field_type}
                        onChange={(v) => setEditingField(s => ({ ...s!, field_type: v || 'UNSET' }))}
                        options={fieldTypes}
                        label={t('customFieldsPage.modal.labelFieldType')}
                        placeholder={t('customFieldsPage.modal.placeholderFieldType')}
                    />

                    <Show when={editingField()?.field_type === 'STRING'}>
                        <TextField
                            label={t('customFieldsPage.modal.labelValue')}
                            placeholder={t('customFieldsPage.modal.placeholderStringValue')}
                            value={editingField()?.string_value || ''}
                            onChange={(v) => setEditingField(s => ({ ...s!, string_value: v }))}
                        />
                    </Show>
                    <Show when={editingField()?.field_type === 'JSON_STRING'}>
                        <TextField
                            label={t('customFieldsPage.modal.labelValue')}
                            placeholder={t('customFieldsPage.modal.placeholderJsonStringValue')}
                            value={editingField()?.string_value || ''}
                            onChange={(v) => setEditingField(s => ({ ...s!, string_value: v }))}
                        />
                    </Show>
                    <Show when={editingField()?.field_type === 'INTEGER'}>
                        <NumberField
                            label={t('customFieldsPage.modal.labelValue')}
                            placeholder={t('customFieldsPage.modal.placeholderIntegerValue')}
                            value={editingField()?.integer_value ?? undefined}
                            onChange={(v) => setEditingField(s => ({ ...s!, integer_value: isNaN(v) ? null : v }))}
                            step={1}
                            formatOptions={{ maximumFractionDigits: 0 }}
                        />
                    </Show>
                    <Show when={editingField()?.field_type === 'NUMBER'}>
                        <NumberField
                            label={t('customFieldsPage.modal.labelValue')}
                            placeholder={t('customFieldsPage.modal.placeholderNumberValue')}
                            value={editingField()?.number_value ?? undefined}
                            onChange={(v) => setEditingField(s => ({ ...s!, number_value: isNaN(v) ? null : v }))}
                        />
                    </Show>
                    <Show when={editingField()?.field_type === 'BOOLEAN'}>
                        <div class="flex items-center space-x-2">
                            <label for="boolean_value_checkbox" class="text-sm font-medium leading-none">{t('customFieldsPage.modal.labelValue')}</label>
                            <input
                                type="checkbox"
                                id="boolean_value_checkbox"
                                class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                                checked={editingField()?.boolean_value || false}
                                onChange={(e) => setEditingField(s => ({ ...s!, boolean_value: e.currentTarget.checked }))}
                            />
                        </div>
                    </Show>

                    <div class="flex items-center space-x-2">
                        <label for="is_enabled_checkbox" class="text-sm font-medium leading-none">{t('customFieldsPage.modal.labelEnabled')}</label>
                        <input
                            type="checkbox"
                            id="is_enabled_checkbox"
                            class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                            checked={editingField()?.is_enabled || false}
                            onChange={(e) => setEditingField(s => ({ ...s!, is_enabled: e.currentTarget.checked }))}
                        />
                    </div>

                    <DialogFooter>
                        <Button onClick={handleCloseModal} variant="secondary">{t('common.cancel')}</Button>
                        <Button onClick={handleSave} variant="primary">{t('common.save')}</Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>
        </div>
    );
}
