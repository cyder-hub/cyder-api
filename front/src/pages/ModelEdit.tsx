import { For, Show, onMount, createSignal, createResource } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import { useI18n } from '../i18n';
import { Button } from '../components/ui/Button';
import { TextField } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '../components/ui/Table';
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
                    <TextField
                        label={<>{t('modelEditPage.labelModelName')} <span class="text-red-500">*</span></>}
                        value={editingData!.model_name}
                        onChange={(v) => updateEditingDataField('model_name', v)}
                    />
                    <TextField
                        label={t('modelEditPage.labelRealModelName')}
                        value={editingData!.real_model_name ?? ''}
                        onChange={(v) => updateEditingDataField('real_model_name', v)}
                    />
                    <div class="flex items-center space-x-2">
                        <input
                            type="checkbox"
                            id="is_enabled_model_checkbox"
                            class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                            checked={editingData!.is_enabled}
                            onChange={(e) => updateEditingDataField('is_enabled', e.currentTarget.checked)}
                        />
                        <label for="is_enabled_model_checkbox" class="text-sm font-medium leading-none">{t('modelEditPage.labelEnabled')}</label>
                    </div>

                    {/* Price Management Section */}
                    <div class="section">
                        <h3 class="section-title">{t('modelEditPage.priceSection.title')}</h3>
                        <Select
                            label={t('modelEditPage.priceSection.labelBillingPlan')}
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
                        />

                        <Show when={editingData!.billing_plan_id}>
                            <div class="mt-4">
                                <Show when={priceRules.loading}>
                                    <p>{t('modelEditPage.priceSection.loadingRules')}</p>
                                </Show>
                                <Show when={!priceRules.loading && priceRules() && priceRules()!.length > 0}>
                                    <h4 class="text-md font-semibold mb-2">{t('modelEditPage.priceSection.rulesTitle')}</h4>
                                    <TableRoot>
                                        <TableHeader>
                                            <TableRow>
                                                <TableColumnHeader>{t('pricePage.rules.table.description')}</TableColumnHeader>
                                                <TableColumnHeader>{t('pricePage.rules.table.enabled')}</TableColumnHeader>
                                                <TableColumnHeader>{t('pricePage.rules.table.usageType')}</TableColumnHeader>
                                                <TableColumnHeader>{t('pricePage.rules.table.mediaType')}</TableColumnHeader>
                                                <TableColumnHeader>{t('pricePage.rules.table.price')}</TableColumnHeader>
                                                <TableColumnHeader>{t('pricePage.rules.table.effectiveFrom')}</TableColumnHeader>
                                            </TableRow>
                                        </TableHeader>
                                        <TableBody>
                                            <For each={priceRules()}>
                                                {(rule) => (
                                                    <TableRow>
                                                        <TableCell>{rule.description}</TableCell>
                                                        <TableCell>{rule.is_enabled ? t('common.yes') : t('common.no')}</TableCell>
                                                        <TableCell>{rule.usage_type}</TableCell>
                                                        <TableCell>{rule.media_type || '-'}</TableCell>
                                                        <TableCell class="text-right">{rule.price_in_micro_units / 1000} {selectedPlan()?.currency}</TableCell>
                                                        <TableCell>{formatTimestamp(rule.effective_from)}</TableCell>
                                                    </TableRow>
                                                )}
                                            </For>
                                        </TableBody>
                                    </TableRoot>
                                </Show>
                            </div>
                        </Show>
                    </div>

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
                                placeholder={t('modelEditPage.placeholderSelectCustomField')}
                            />
                            <Button variant="primary" size="sm" onClick={handleLinkCustomField} disabled={!selectedCustomFieldId()}>
                                {t('modelEditPage.buttonAddCustomField')}
                            </Button>
                        </div>
                    </div>

                    <div class="mt-6 flex justify-end space-x-2 pt-4 border-t">
                        <Button variant="secondary" onClick={handleNavigateBack}>{t('common.cancel')}</Button>
                        <Button variant="primary" onClick={handleSaveModel}>{t('common.save')}</Button>
                    </div>
                </div>
            </Show>
        </div>
    );
}
