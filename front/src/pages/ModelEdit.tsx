import { For, Show, onMount, createSignal, createResource } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import { useI18n } from '../i18n';
import { Button } from '@kobalte/core/button';
import { TextField } from '@kobalte/core/text-field';
import { Checkbox } from '@kobalte/core/checkbox';
import { Select } from '@kobalte/core/select';
import { useNavigate, useParams } from '@solidjs/router';
import { request } from '../services/api';
import { toastController } from '../components/GlobalMessage';
import type { CustomFieldType } from '../store/types';
import { refetchProviders as globalRefetchProviders } from '../store/providerStore';

interface BillingPlan {
    id: number;
    name: string;
    currency: string;
}

interface PriceRule {
    id: number;
    plan_id: number;
    description: string | null;
    is_enabled: boolean;
    effective_from: number;
    effective_until: number | null;
    usage_type: string;
    media_type: string | null;
    price_in_micro_units: number;
}

// Local interface for custom fields, must include id for linking/unlinking
interface CustomFieldItem {
    id: number;
    field_name: string;
    field_value: string;
    description: string | null;
    field_type: CustomFieldType;
}

// Interface for the data being edited
interface EditingModelData {
    id: number;
    provider_id: number;
    billing_plan_id: number | null;
    model_name: string;
    real_model_name: string | null;
    is_enabled: boolean;
    custom_fields: CustomFieldItem[];
}

interface ModelItem {
    id: number;
    provider_id: number;
    billing_plan_id: number | null;
    model_name: string;
    real_model_name: string | null;
    is_enabled: boolean;
}

interface ModelDetailFromApi {
    model: ModelItem;
    custom_fields: CustomFieldItem[];
}

const fetchBillingPlans = async (): Promise<BillingPlan[]> => {
    try {
        const response = await request('/ai/manager/api/price/plan/list');
        return response || [];
    } catch (error) {
        console.error("Failed to fetch billing plans", error);
        toastController.error("Failed to fetch billing plans");
        return [];
    }
};

const fetchPriceRules = async (planId: number | null): Promise<PriceRule[]> => {
    if (!planId) return [];
    try {
        const response = await request(`/ai/manager/api/price/rule/list_by_plan?plan_id=${planId}`);
        return response || [];
    } catch (error) {
        console.error(`Failed to fetch price rules for plan ${planId}`, error);
        toastController.error(`Failed to fetch price rules for plan ${planId}`);
        return [];
    }
};

const fetchModelDetail = async (modelId: number): Promise<ModelDetailFromApi | null> => {
    try {
        const response = await request(`/ai/manager/api/model/${modelId}/detail`);
        return response || null;
    } catch (error) {
        console.error(`Failed to fetch model detail for ID ${modelId}`, error);
        return null;
    }
};

const fetchAllCustomFields = async (t: (key: string) => string): Promise<CustomFieldItem[]> => {
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
        toastController.error(t('modelEditPage.alert.fetchCustomFieldsFailed'));
        return [];
    }
};

const formatTimestamp = (ms: number | undefined | null): string => {
    if (!ms) return '';
    const date = new Date(ms);
    return date.toLocaleString();
};

export default function ModelEdit() {
    const navigate = useNavigate();
    const params = useParams();
    const modelId = params.id ? parseInt(params.id) : null;
    const [t] = useI18n();

    const [editingData, setEditingData] = createStore<EditingModelData | null>(null);
    const [allCustomFields, setAllCustomFields] = createSignal<CustomFieldItem[]>([]);
    const [selectedCustomFieldId, setSelectedCustomFieldId] = createSignal<number | null>(null);
    const [isLoading, setIsLoading] = createSignal<boolean>(true);
    const [error, setError] = createSignal<string | null>(null);

    const [billingPlans] = createResource(fetchBillingPlans);
    const [priceRules] = createResource(() => editingData?.billing_plan_id, fetchPriceRules);

    const selectedPlan = () => billingPlans()?.find(p => p.id === editingData?.billing_plan_id);

    const reloadData = async () => {
        if (!modelId) {
            setError(t('modelEditPage.alert.missingId'));
            setIsLoading(false);
            return;
        }

        setIsLoading(true);
        setError(null);

        const fields = await fetchAllCustomFields(t);
        setAllCustomFields(fields);

        const detail = await fetchModelDetail(modelId);
        if (detail) {
            setEditingData({
                id: detail.model.id,
                provider_id: detail.model.provider_id,
                billing_plan_id: detail.model.billing_plan_id ?? null,
                model_name: detail.model.model_name,
                real_model_name: detail.model.real_model_name ?? null,
                is_enabled: detail.model.is_enabled,
                custom_fields: (detail.custom_fields || []).map(f => ({
                    id: f.id,
                    field_name: f.field_name,
                    field_value: (f.string_value ?? f.integer_value?.toString() ?? f.number_value?.toString() ?? f.boolean_value?.toString()) || '',
                    description: f.description,
                    field_type: (f.field_type?.toLowerCase() as CustomFieldType) || 'unset'
                })),
            });
        } else {
            setError(t('modelEditPage.alert.loadDataFailed', { modelId: modelId }));
        }
        setIsLoading(false);
    };

    onMount(reloadData);

    const handleNavigateBack = () => {
        navigate('/provider'); // Navigate back to provider list
    };

    const handleSaveModel = async () => {
        const currentData = editingData;
        if (!currentData) return;

        if (!currentData.model_name.trim()) {
            toastController.warn(t('modelEditPage.alert.nameRequired'));
            return;
        }

        const payload = {
            model_name: currentData.model_name,
            real_model_name: currentData.real_model_name || null,
            is_enabled: currentData.is_enabled,
            billing_plan_id: currentData.billing_plan_id,
        };

        try {
            await request(`/ai/manager/api/model/${currentData.id}`, {
                method: 'PUT',
                body: JSON.stringify(payload)
            });
            toastController.success(t('modelEditPage.alert.updateSuccess'));
            globalRefetchProviders(); // To update provider list which shows models
            await reloadData();
        } catch (error) {
            console.error("Failed to save model:", error);
            toastController.error(t('modelEditPage.alert.saveFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    const updateEditingDataField = (field: keyof EditingModelData, value: any) => {
        if (editingData) {
            setEditingData(field, value);
        }
    };

    const availableCustomFields = () => {
        if (!editingData) return [];
        const linkedIds = new Set(editingData.custom_fields.map(f => f.id));
        return allCustomFields().filter(f => f.id && !linkedIds.has(f.id));
    };

    const handleLinkCustomField = async () => {
        const field = selectedCustomFieldId();
        const modelId = editingData?.id;

        if (!field) {
            toastController.warn(t('modelEditPage.alert.selectFieldToLink'));
            return;
        }
        if (!modelId) {
            toastController.warn(t('modelEditPage.alert.modelNotLoaded'));
            return;
        }

        const fieldId = (field as any).id ?? field;

        try {
            await request('/ai/manager/api/custom_field_definition/link', {
                method: 'POST',
                body: JSON.stringify({
                    custom_field_definition_id: fieldId,
                    model_id: modelId,
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
            toastController.success(t('modelEditPage.alert.linkSuccess'));
        } catch (error) {
            console.error("Failed to link custom field:", error);
            toastController.error(t('modelEditPage.alert.linkFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };


    const handleUnlinkCustomField = async (fieldId: number, index: number) => {
        const modelId = editingData?.id;
        if (!modelId) {
            toastController.warn(t('modelEditPage.alert.modelIdNotFound'));
            return;
        }

        try {
            await request('/ai/manager/api/custom_field_definition/unlink', {
                method: 'POST',
                body: JSON.stringify({
                    custom_field_definition_id: fieldId,
                    model_id: modelId,
                }),
            });

            setEditingData('custom_fields', produce(fields => {
                fields.splice(index, 1);
            }));
            toastController.success(t('modelEditPage.alert.unlinkSuccess'));
        } catch (error) {
            console.error("Failed to unlink custom field:", error);
            toastController.error(t('modelEditPage.alert.unlinkFailed', { error: (error as Error).message || t('unknownError') }));
        }
    };

    return (
        <div class="p-4 space-y-6 bg-white rounded-lg shadow-xl max-w-3xl mx-auto my-8">
            <h1 class="text-2xl font-semibold mb-4 text-gray-800">{t('modelEditPage.title')}</h1>
            <Show when={isLoading()}>
                <div class="text-center py-4 text-gray-500">{t('modelEditPage.loading')}</div>
            </Show>
            <Show when={error()}>
                <div class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4">
                    {error()}
                </div>
            </Show>
            <Show when={!isLoading() && !error() && editingData}>
                <div class="space-y-4">
                    <TextField class="form-item" value={editingData!.model_name} onChange={(v) => updateEditingDataField('model_name', v)}>
                        <TextField.Label class="form-label">{t('modelEditPage.labelModelName')} <span class="text-red-500">*</span></TextField.Label>
                        <TextField.Input class="form-input" />
                    </TextField>
                    <TextField class="form-item" value={editingData!.real_model_name ?? ''} onChange={(v) => updateEditingDataField('real_model_name', v)}>
                        <TextField.Label class="form-label">{t('modelEditPage.labelRealModelName')}</TextField.Label>
                        <TextField.Input class="form-input" />
                    </TextField>
                    <Checkbox class="form-item items-center" checked={editingData!.is_enabled} onChange={(v) => updateEditingDataField('is_enabled', v)}>
                        <Checkbox.Input class="form-checkbox" />
                        <Checkbox.Label class="form-label ml-2">{t('modelEditPage.labelEnabled')}</Checkbox.Label>
                    </Checkbox>

                    {/* Custom Fields Section */}
                    <div class="section">
                        <h3 class="section-title">{t('modelEditPage.sectionCustomFields')}</h3>
                        <div class="section-header grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-2 items-center">
                            <span class="font-semibold">{t('modelEditPage.tableHeaderFieldName')}</span>
                            <span class="font-semibold">{t('modelEditPage.tableHeaderFieldValue')}</span>
                            <span class="font-semibold">{t('modelEditPage.tableHeaderDescription')}</span>
                            <span class="font-semibold">{t('modelEditPage.tableHeaderFieldType')}</span>
                            <span></span>
                        </div>
                        <For each={editingData!.custom_fields}>
                            {(field, index) => (
                                <div class="section-row grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-2 items-center mb-2">
                                    <TextField value={field.field_name} disabled>
                                        <TextField.Input class="form-input" />
                                    </TextField>
                                    <TextField value={field.field_value} disabled>
                                        <TextField.Input class="form-input" />
                                    </TextField>
                                    <TextField value={field.description ?? ''} disabled>
                                        <TextField.Input class="form-input" />
                                    </TextField>
                                    <TextField value={field.field_type} disabled>
                                        <TextField.Input class="form-input" />
                                    </TextField>
                                    <Button class="btn btn-danger btn-sm" onClick={() => handleUnlinkCustomField(field.id, index())}>{t('common.delete')}</Button>
                                </div>
                            )}
                        </For>
                        <div class="mt-4 flex items-center gap-2">
                            <Select<CustomFieldItem>
                                value={selectedCustomFieldId()}
                                onChange={setSelectedCustomFieldId}
                                options={availableCustomFields()}
                                optionValue="id"
                                optionTextValue="field_name"
                                placeholder={t('modelEditPage.placeholderSelectCustomField')}
                                itemComponent={props => (
                                    <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                                        <Select.ItemLabel>{props.item.rawValue.field_name}</Select.ItemLabel>
                                    </Select.Item>
                                )}
                            >
                                <Select.Trigger class="form-select w-full" aria-label={t('modelEditPage.labelSelectCustomField')}>
                                    <Select.Value<CustomFieldItem>>{state => state.selectedOption() ? state.selectedOption().field_name : ''}</Select.Value>
                                    <Select.Icon class="ml-2 text-gray-500">▼</Select.Icon>
                                </Select.Trigger>
                                <Select.Portal>
                                    <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                                        <Select.Listbox class="max-h-60 overflow-y-auto py-1" />
                                    </Select.Content>
                                </Select.Portal>
                            </Select>
                            <Button class="btn btn-primary btn-sm" onClick={handleLinkCustomField} disabled={!selectedCustomFieldId()}>
                                {t('modelEditPage.buttonAddCustomField')}
                            </Button>
                        </div>
                    </div>

                    {/* Price Management Section */}
                    <div class="section">
                        <h3 class="section-title">{t('modelEditPage.priceSection.title')}</h3>
                        <Select<BillingPlan | { id: null, name: string }>
                            value={
                                [
                                    { id: null, name: t('modelEditPage.priceSection.noPlan') },
                                    ...(billingPlans() || [])
                                ].find(p => p.id === editingData!.billing_plan_id)
                            }
                            onChange={(v) => updateEditingDataField('billing_plan_id', v?.id ?? null)}
                            options={[
                                { id: null, name: t('modelEditPage.priceSection.noPlan') },
                                ...(billingPlans() || [])
                            ]}
                            optionValue="id"
                            optionTextValue="name"
                            placeholder={t('modelEditPage.priceSection.placeholderBillingPlan')}
                            itemComponent={props => (
                                <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                                    <Select.ItemLabel>{props.item.rawValue.name}</Select.ItemLabel>
                                </Select.Item>
                            )}
                        >
                            <Select.Label class="form-label">{t('modelEditPage.priceSection.labelBillingPlan')}</Select.Label>
                            <Select.Trigger class="form-select w-full" aria-label={t('modelEditPage.priceSection.labelBillingPlan')}>
                                <Select.Value<BillingPlan | { id: null, name: string }>>
                                    {state => state.selectedOption() ? state.selectedOption().name : t('modelEditPage.priceSection.noPlan')}
                                </Select.Value>
                                <Select.Icon class="ml-2 text-gray-500">▼</Select.Icon>
                            </Select.Trigger>
                            <Select.Portal>
                                <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                                    <Select.Listbox class="max-h-60 overflow-y-auto py-1" />
                                </Select.Content>
                            </Select.Portal>
                        </Select>

                        <Show when={editingData!.billing_plan_id}>
                            <div class="mt-4">
                                <Show when={priceRules.loading}>
                                    <p>{t('modelEditPage.priceSection.loadingRules')}</p>
                                </Show>
                                <Show when={!priceRules.loading && priceRules() && priceRules()!.length > 0}>
                                    <h4 class="text-md font-semibold mb-2">{t('modelEditPage.priceSection.rulesTitle')}</h4>
                                    <table class="data-table min-w-full text-sm border-t">
                                        <thead class="bg-gray-50">
                                            <tr>
                                                <th class="px-3 py-2 text-left font-medium text-gray-500">{t('pricePage.rules.table.description')}</th>
                                                <th class="px-3 py-2 text-left font-medium text-gray-500">{t('pricePage.rules.table.enabled')}</th>
                                                <th class="px-3 py-2 text-left font-medium text-gray-500">{t('pricePage.rules.table.usageType')}</th>
                                                <th class="px-3 py-2 text-left font-medium text-gray-500">{t('pricePage.rules.table.mediaType')}</th>
                                                <th class="px-3 py-2 text-left font-medium text-gray-500">{t('pricePage.rules.table.price')}</th>
                                                <th class="px-3 py-2 text-left font-medium text-gray-500">{t('pricePage.rules.table.effectiveFrom')}</th>
                                            </tr>
                                        </thead>
                                        <tbody class="bg-white divide-y divide-gray-200">
                                            <For each={priceRules()}>
                                                {(rule) => (
                                                    <tr>
                                                        <td class="px-3 py-2 whitespace-nowrap">{rule.description}</td>
                                                        <td class="px-3 py-2">{rule.is_enabled ? t('common.yes') : t('common.no')}</td>
                                                        <td class="px-3 py-2">{rule.usage_type}</td>
                                                        <td class="px-3 py-2">{rule.media_type || '-'}</td>
                                                        <td class="px-3 py-2 text-right">{rule.price_in_micro_units / 1000} {selectedPlan()?.currency}</td>
                                                        <td class="px-3 py-2">{formatTimestamp(rule.effective_from)}</td>
                                                    </tr>
                                                )}
                                            </For>
                                        </tbody>
                                    </table>
                                </Show>
                            </div>
                        </Show>
                    </div>

                    <div class="mt-6 flex justify-end space-x-2 pt-4 border-t">
                        <Button class="btn btn-secondary" onClick={handleNavigateBack}>{t('common.cancel')}</Button>
                        <Button class="btn btn-primary" onClick={handleSaveModel}>{t('common.save')}</Button>
                    </div>
                </div>
            </Show>
            {/* Styles (copied from ProviderEdit.tsx) */}
            <style jsx global>{`
                .section {
                    margin-bottom: 1.25rem; /* 20px */
                    padding: 0.75rem; /* 12px */
                    border: 1px solid #e5e7eb; /* gray-200 */
                    border-radius: 0.375rem; /* rounded-md */
                }
                .section-title {
                    font-size: 1.125rem; /* text-lg */
                    font-weight: 600; /* font-semibold */
                    margin-bottom: 0.5rem; /* 8px */
                }
                .required-field::after {
                    content: "*";
                    color: #ef4444; /* red-500 */
                    margin-left: 0.25rem; /* 4px */
                }
                .form-item { margin-bottom: 1rem; }
                .form-label { display: block; margin-bottom: 0.25rem; font-weight: 500; color: #374151; /* gray-700 */ }
                .form-input, .form-select {
                    width: 100%;
                    padding: 0.5rem 0.75rem;
                    border: 1px solid #d1d5db; /* gray-300 */
                    border-radius: 0.375rem; /* rounded-md */
                    box-shadow: inset 0 1px 2px 0 rgba(0,0,0,0.05);
                }
                .form-input:focus, .form-select:focus {
                    border-color: #2563eb; /* blue-600 */
                    outline: 2px solid transparent;
                    outline-offset: 2px;
                    box-shadow: 0 0 0 2px #bfdbfe; /* blue-200 */
                }
                .form-checkbox {
                    border-radius: 0.25rem;
                    border-color: #d1d5db; /* gray-300 */
                }
                .form-checkbox:focus {
                     border-color: #2563eb; /* blue-600 */
                     box-shadow: 0 0 0 2px #bfdbfe; /* blue-200 */
                }
                .btn {
                    padding: 0.5rem 1rem;
                    border-radius: 0.375rem;
                    font-weight: 500;
                    transition: background-color 0.15s ease-in-out;
                    box-shadow: 0 1px 2px 0 rgba(0,0,0,0.05);
                }
                .btn-sm { padding: 0.25rem 0.75rem; font-size: 0.875rem; }
                .btn-primary { background-color: #2563eb; color: white; }
                .btn-primary:hover { background-color: #1d4ed8; }
                .btn-secondary { background-color: #6b7280; color: white; }
                .btn-secondary:hover { background-color: #4b5563; }
                .btn-danger { background-color: #dc2626; color: white; }
                .btn-danger:hover { background-color: #b91c1c; }
                .btn-success { background-color: #16a34a; color: white; }
                .btn-success:hover { background-color: #15803d; }
                .btn-info { background-color: #3b82f6; color: white; } /* blue-500 */
                .btn-info:hover { background-color: #2563eb; } /* blue-600 */
                .kb-select__trigger.form-select {
                     /* padding already handled by .form-select */
                }
            `}</style>
        </div>
    );
}
