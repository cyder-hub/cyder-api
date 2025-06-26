import { createSignal, For, Show, createResource, Accessor, Setter, onMount } from 'solid-js';
import type { Resource } from 'solid-js';
import { Button } from '@kobalte/core/button';
import { Select as KSelect } from '@kobalte/core/select'; // Aliased to avoid conflict if HTMLSelectElement is used
import { TextField } from '@kobalte/core/text-field';
import { request } from '../services/api';
import { useI18n } from '../i18n';

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

// UI representation of an AccessControlRule
interface AccessControlRuleUI {
    id: number | null; // null for new rules
    rule_type: 'ALLOW' | 'DENY';
    priority: number;
    scope: 'PROVIDER' | 'MODEL';
    provider_id: number | null;
    model_id: number | null;
    description: string | null;
    is_enabled: boolean;
}

interface AccessControlPolicyBase {
    name: string;
    default_action: 'ALLOW' | 'DENY';
    description: string | null;
}

// Represents a full policy in the UI for editing
interface AccessControlPolicyUI extends AccessControlPolicyBase {
    id: number | null; // null for new policy
    rules: AccessControlRuleUI[];
}

// API response for a single policy (summary or detail)
export interface AccessControlPolicyFromAPI extends AccessControlPolicyBase {
    id: number;
    created_at: number;
    updated_at: number;
    rules: Array<{
        id: number;
        policy_id: number;
        rule_type: string;
        priority: number;
        scope: string;
        provider_id: number | null;
        model_id: number | null;
        is_enabled: boolean;
        description: string | null;
        created_at: number;
        updated_at: number;
        deleted_at: number | null;
    }>;
}

const newPolicyTemplate = (): AccessControlPolicyUI => ({
    id: null,
    name: '',
    default_action: 'ALLOW',
    description: '',
    rules: []
});

const newRuleTemplate = (): AccessControlRuleUI => ({
    id: null,
    rule_type: 'ALLOW',
    priority: 0,
    scope: 'PROVIDER',
    provider_id: null,
    model_id: null,
    description: '',
    is_enabled: true,
});

// --- API Functions ---

export const fetchPoliciesAPI = async (): Promise<AccessControlPolicyFromAPI[]> => {
    try {
        const response = await request("/ai/manager/api/access_control/list");
        return response || [];
    } catch (error) {
        console.error("Failed to fetch policies:", error);
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

const fetchPolicyDetailAPI = async (id: number): Promise<AccessControlPolicyFromAPI | null> => {
    try {
        const response = await request(`/ai/manager/api/access_control/${id}`);
        return response || null;
    } catch (error) {
        console.error("Failed to fetch policy detail:", error);
        return null;
    }
};

const savePolicyAPI = async (policy: AccessControlPolicyUI): Promise<any> => {
    const payload = {
        name: policy.name,
        default_action: policy.default_action,
        description: policy.description || null,
        rules: policy.rules.map(rule => ({
            rule_type: rule.rule_type,
            priority: Number(rule.priority) || 0,
            scope: rule.scope,
            provider_id: rule.provider_id,
            model_id: rule.scope === 'MODEL' ? rule.model_id : null,
            description: rule.description || null,
            is_enabled: rule.is_enabled,
        })),
    };

    const url = policy.id ? `/ai/manager/api/access_control/${policy.id}` : '/ai/manager/api/access_control';
    const method = policy.id ? 'PUT' : 'POST';

    return request(url, {
        method: method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
    });
};

const deletePolicyAPI = async (id: number): Promise<any> => {
    return request(`/ai/manager/api/access_control/${id}`, { method: 'DELETE' });
};


// --- Helper to transform API detail to UI state ---
function transformApiDetailToUiState(apiDetail: AccessControlPolicyFromAPI): AccessControlPolicyUI {
    return {
        id: apiDetail.id,
        name: apiDetail.name,
        default_action: (apiDetail.default_action as string).toUpperCase() as 'ALLOW' | 'DENY',
        description: apiDetail.description,
        rules: (apiDetail.rules || []).map(rule => ({
            id: rule.id,
            rule_type: rule.rule_type.toUpperCase() as 'ALLOW' | 'DENY',
            priority: rule.priority,
            scope: rule.scope.toUpperCase() as 'PROVIDER' | 'MODEL',
            provider_id: rule.provider_id,
            model_id: rule.model_id,
            description: rule.description,
            is_enabled: rule.is_enabled,
        })).sort((a, b) => a.priority - b.priority), // Sort by priority for consistent display
    };
}


export default function AccessControlPage() {
    const [t] = useI18n();

    // --- Tailwind CSS classes for form elements ---
    const formLabelClass = "block mb-1 text-sm font-medium text-gray-700";
    const formLabelSmClass = "text-xs";
    const formControlBaseClass = "block w-full border border-gray-300 rounded-md transition";
    const formControlFocusRingClass = "focus:border-blue-600 focus:outline-none focus:ring-2 focus:ring-blue-600/[.25]";
    const formControlFocusWithinRingClass = "focus-within:border-blue-600 focus-within:outline-none focus-within:ring-2 focus-within:ring-blue-600/[.25]";
    const formInputClass = `${formControlBaseClass} py-2 px-3 text-base ${formControlFocusRingClass}`;
    const formInputSmClass = `${formControlBaseClass} py-1 px-2 text-sm ${formControlFocusRingClass}`;
    const formSelectTriggerClass = `w-full ${formControlBaseClass} py-2 px-3 text-base ${formControlFocusWithinRingClass}`;
    const formSelectTriggerSmClass = `w-full ${formControlBaseClass} py-1 px-2 text-sm ${formControlFocusWithinRingClass}`;
    const kSelectItemClass = "flex justify-between items-center py-[0.375rem] px-3 text-sm text-gray-700 cursor-default data-[highlighted]:bg-blue-100 data-[highlighted]:text-blue-700 data-[selected]:font-semibold";
    const kSelectContentClass = "bg-white border border-gray-300 rounded-md shadow-lg mt-1 z-50 max-h-60 overflow-y-auto";

    const [policies, { refetch: refetchPolicies }] = createResource<AccessControlPolicyFromAPI[]>(fetchPoliciesAPI, { initialValue: [] });
    const [providers] = createResource<ProviderDetail[]>(fetchProvidersWithModelsAPI, { initialValue: [] });

    const [showEditModal, setShowEditModal] = createSignal(false);
    const [editingPolicy, setEditingPolicy] = createSignal<AccessControlPolicyUI>(newPolicyTemplate());

    const handleOpenAddModal = () => {
        setEditingPolicy(newPolicyTemplate());
        setShowEditModal(true);
    };

    const handleOpenEditModal = async (id: number) => {
        const detail = await fetchPolicyDetailAPI(id);
        if (detail) {
            setEditingPolicy(transformApiDetailToUiState(detail));
            setShowEditModal(true);
        } else {
            alert(t('accessControlPage.alert.loadDetailFailed'));
        }
    };

    const handleCloseModal = () => {
        setShowEditModal(false);
    };

    const handleSavePolicy = async () => {
        const policy = editingPolicy();
        if (!policy) return;
        if (!policy.name) {
            alert(t('accessControlPage.alert.nameRequired'));
            return;
        }

        try {
            await savePolicyAPI(policy);
            setShowEditModal(false);
            refetchPolicies();
        } catch (error) {
            console.error("Failed to save policy:", error);
            alert(t('accessControlPage.alert.saveFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
        }
    };

    const handleDeletePolicy = async (id: number, name: string) => {
        if (confirm(t('accessControlPage.confirmDelete', { name }))) {
            try {
                await deletePolicyAPI(id);
                refetchPolicies();
            } catch (error) {
                console.error("Failed to delete policy:", error);
                alert(t('accessControlPage.alert.deleteFailed', { error: error instanceof Error ? error.message : t('unknownError') }));
            }
        }
    };

    // --- Rule Management ---
    const addRule = () => {
        const currentPolicy = editingPolicy();
        if (!currentPolicy) return;
        setEditingPolicy({
            ...currentPolicy,
            rules: [...currentPolicy.rules, newRuleTemplate()]
        });
    };

    const removeRule = (index: number) => {
        const currentPolicy = editingPolicy();
        if (!currentPolicy) return;
        const updatedRules = [...currentPolicy.rules];
        updatedRules.splice(index, 1);
        setEditingPolicy({
            ...currentPolicy,
            rules: updatedRules
        });
    };

    const updateRuleField = <T extends keyof AccessControlRuleUI>(index: number, field: T, value: AccessControlRuleUI[T]) => {
        const currentPolicy = editingPolicy();
        if (!currentPolicy) return;

        const updatedRules = currentPolicy.rules.map((rule, i) =>
            i === index ? { ...rule, [field]: value } : rule
        );

        // Reset dependent fields on scope change
        if (field === 'scope') {
            const updatedRule = updatedRules[index];
            if (value === 'PROVIDER') {
                updatedRule.model_id = null;
            }
        } else if (field === 'provider_id') {
            const updatedRule = updatedRules[index];
            updatedRule.model_id = null; // Reset model if provider changes
        }

        setEditingPolicy({ ...currentPolicy, rules: updatedRules });
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
        return (pDetail?.models || []).map(mDetail => mDetail.model);
    };


    const getModelById = (providerId: number | null, modelId: number | null): Model | null => {
        if (modelId === null || providerId === null) return null;
        const models = getModelsForProvider(providerId);
        const model = models.find(m => m.id === modelId);
        return model || null;
    };


    return (
        <div class="p-4 space-y-6">
            <div class="flex justify-between items-center mb-4">
                <h1 class="text-2xl font-semibold text-gray-800">{t('accessControlPage.title')}</h1>
                <Button onClick={handleOpenAddModal} class="btn btn-primary">{t('accessControlPage.addPolicy')}</Button>
            </div>

            {/* Data Table */}
            <Show when={policies.loading}>
                <div class="text-center py-4 text-gray-500">{t('accessControlPage.loading')}</div>
            </Show>
            <Show when={!policies.loading && policies.error}>
                <div class="text-center py-4 text-red-500">{t('accessControlPage.error')}</div>
            </Show>
            <Show when={!policies.loading && !policies.error && policies()?.length === 0}>
                <div class="text-center py-4 text-gray-500">{t('accessControlPage.noData')}</div>
            </Show>
            <Show when={!policies.loading && !policies.error && policies() && policies()!.length > 0}>
                <div class="overflow-x-auto shadow-md rounded-lg border border-gray-200">
                    <table class="min-w-full divide-y divide-gray-200 data-table">
                        <thead class="bg-gray-100">
                            <tr>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('accessControlPage.table.name')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('accessControlPage.table.defaultAction')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('accessControlPage.table.description')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('accessControlPage.table.rules')}</th>
                                <th class="px-4 py-3 text-left text-xs font-medium text-gray-600 uppercase tracking-wider">{t('accessControlPage.table.actions')}</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">
                            <For each={policies()}>{(policy) =>
                                <tr>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-800">{policy.name}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{t(`accessControlPage.modal.option${policy.default_action}`)}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{policy.description || '/'}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm text-gray-600">{t('accessControlPage.table.rulesCount', { count: policy.rules?.length || 0 })}</td>
                                    <td class="px-4 py-3 whitespace-nowrap text-sm space-x-2">
                                        <Button onClick={() => handleOpenEditModal(policy.id)} class="btn btn-primary btn-sm">{t('common.edit')}</Button>
                                        <Button onClick={() => handleDeletePolicy(policy.id, policy.name)} class="btn btn-danger btn-sm">{t('common.delete')}</Button>
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
                    <div class="bg-white rounded-lg shadow-xl p-6 space-y-4 w-full max-w-4xl max-h-[90vh] overflow-y-auto model"> {/* Increased max-w */}
                        <h2 class="text-xl font-semibold text-gray-800 model-title">{editingPolicy()?.id ? t('accessControlPage.modal.titleEdit') : t('accessControlPage.modal.titleAdd')}</h2>

                        {/* Policy Fields */}
                        <TextField class="form-item" value={editingPolicy()?.name || ''} onChange={(v) => setEditingPolicy(p => ({ ...p!, name: v }))}>
                            <TextField.Label class={formLabelClass}>{t('accessControlPage.modal.labelName')}</TextField.Label>
                            <TextField.Input class={formInputClass} />
                        </TextField>
                        <div class="form-item">
                            <KSelect<string>
                                value={editingPolicy()?.default_action || 'ALLOW'}
                                onChange={(v) => setEditingPolicy(p => ({ ...p!, default_action: v as 'ALLOW' | 'DENY' }))}
                                options={['ALLOW', 'DENY']}
                                placeholder={t('accessControlPage.modal.placeholderDefaultAction')}
                                itemComponent={props => (
                                    <KSelect.Item item={props.item} class={kSelectItemClass}>
                                        <KSelect.ItemLabel>{t(`accessControlPage.modal.option${props.item.rawValue}`)}</KSelect.ItemLabel>
                                        <KSelect.ItemIndicator>✓</KSelect.ItemIndicator>
                                    </KSelect.Item>
                                )}
                            >
                                <KSelect.Label class={formLabelClass}>{t('accessControlPage.modal.labelDefaultAction')}</KSelect.Label>
                                <KSelect.Trigger class={formSelectTriggerClass}>
                                    <KSelect.Value<string>>{state => t(`accessControlPage.modal.option${state.selectedOption()}`)}</KSelect.Value>
                                </KSelect.Trigger>
                                <KSelect.Portal><KSelect.Content class={kSelectContentClass}><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                            </KSelect>
                        </div>
                        <TextField class="form-item" value={editingPolicy()?.description || ''} onChange={(v) => setEditingPolicy(p => ({ ...p!, description: v }))}>
                            <TextField.Label class={formLabelClass}>{t('accessControlPage.modal.labelDescription')}</TextField.Label>
                            <TextField.Input as="textarea" rows={2} class={formInputClass} />
                        </TextField>

                        <hr class="my-6" />

                        {/* Rules Section */}
                        <div class="space-y-3">
                            <div class="flex justify-between items-center">
                                <h4 class="text-lg font-medium">{t('accessControlPage.rules.title')}</h4>
                                <Button onClick={addRule} class="btn btn-secondary btn-sm">{t('accessControlPage.rules.addRule')}</Button>
                            </div>
                            <div class="max-h-80 overflow-y-auto border rounded p-2 space-y-2">
                                <Show when={editingPolicy()?.rules.length === 0}>
                                    <p class="text-sm text-gray-500 text-center py-2">{t('accessControlPage.rules.noRules')}</p>
                                </Show>
                                <For each={editingPolicy()?.rules}>{(rule, index) =>
                                    <div class="p-3 border rounded bg-gray-50 space-y-2">
                                        <div class="grid grid-cols-1 md:grid-cols-3 lg:grid-cols-6 gap-2 items-end">
                                            {/* Rule Type */}
                                            <div>
                                                <KSelect<AccessControlRuleUI['rule_type']>
                                                    value={rule.rule_type}
                                                    onChange={(v) => updateRuleField(index(), 'rule_type', v)}
                                                    options={['ALLOW', 'DENY']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class={kSelectItemClass}><KSelect.ItemLabel>{t(`accessControlPage.modal.option${props.item.rawValue}`)}</KSelect.ItemLabel></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class={`${formLabelClass} ${formLabelSmClass}`}>{t('accessControlPage.rules.labelRuleType')}</KSelect.Label>
                                                    <KSelect.Trigger class={formSelectTriggerSmClass}><KSelect.Value<string>>{state => t(`accessControlPage.modal.option${state.selectedOption()}`)}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class={kSelectContentClass}><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            {/* Scope */}
                                            <div>
                                                <KSelect<AccessControlRuleUI['scope']>
                                                    value={rule.scope}
                                                    onChange={(v) => updateRuleField(index(), 'scope', v)}
                                                    options={['PROVIDER', 'MODEL']}
                                                    itemComponent={props => (<KSelect.Item item={props.item} class={kSelectItemClass}><KSelect.ItemLabel>{t(`accessControlPage.rules.scope${props.item.rawValue}`)}</KSelect.ItemLabel></KSelect.Item>)}
                                                >
                                                    <KSelect.Label class={`${formLabelClass} ${formLabelSmClass}`}>{t('accessControlPage.rules.labelScope')}</KSelect.Label>
                                                    <KSelect.Trigger class={formSelectTriggerSmClass}><KSelect.Value<string>>{state => t(`accessControlPage.rules.scope${state.selectedOption()}`)}</KSelect.Value></KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class={kSelectContentClass}><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            {/* Provider */}
                                            <div classList={{ "lg:col-span-2": rule.scope === 'PROVIDER' }}>
                                                <KSelect<ProviderDetail>
                                                    value={providers()?.find(p => p.provider.id === rule.provider_id)}
                                                    onChange={(v: ProviderDetail | null) => updateRuleField(index(), 'provider_id', v ? v.provider.id : null)}
                                                    options={providers() || []}
                                                    optionValue={item => item.provider.id}
                                                    optionTextValue={item => item.provider.name}
                                                    placeholder={t('accessControlPage.rules.placeholderProvider')}
                                                    itemComponent={props => (
                                                        <KSelect.Item item={props.item} class={kSelectItemClass}>
                                                            <KSelect.ItemLabel>{(props.item.rawValue as ProviderDetail).provider.name}</KSelect.ItemLabel>
                                                        </KSelect.Item>
                                                    )}
                                                >
                                                    <KSelect.Label class={`${formLabelClass} ${formLabelSmClass}`}>{t('accessControlPage.rules.labelProvider')}</KSelect.Label>
                                                    <KSelect.Trigger class={formSelectTriggerSmClass}>
                                                        <KSelect.Value<ProviderDetail>>
                                                            {state => state.selectedOption()?.provider.name || t('accessControlPage.rules.placeholderProvider')}
                                                        </KSelect.Value>
                                                    </KSelect.Trigger>
                                                    <KSelect.Portal><KSelect.Content class={kSelectContentClass}><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                </KSelect>
                                            </div>
                                            {/* Model */}
                                            <Show when={rule.scope === 'MODEL'}>
                                                <div>
                                                    <KSelect<Model | null>
                                                        value={getModelById(rule.provider_id, rule.model_id)}
                                                        onChange={(v) => updateRuleField(index(), 'model_id', v ? v.id : null)}
                                                        options={getModelsForProvider(rule.provider_id)}
                                                        optionValue="id"
                                                        optionTextValue={item => item.model_name}
                                                        placeholder={t('accessControlPage.rules.placeholderModel')}
                                                        disabled={!rule.provider_id}
                                                        itemComponent={props => (<KSelect.Item item={props.item} class={kSelectItemClass}><KSelect.ItemLabel>{props.item.rawValue.model_name}</KSelect.ItemLabel></KSelect.Item>)}
                                                    >
                                                        <KSelect.Label class={`${formLabelClass} ${formLabelSmClass}`}>{t('accessControlPage.rules.labelModel')}</KSelect.Label>
                                                        <KSelect.Trigger class={formSelectTriggerSmClass}><KSelect.Value<Model>>{state => state.selectedOption()?.model_name || t('accessControlPage.rules.placeholderModel')}</KSelect.Value></KSelect.Trigger>
                                                        <KSelect.Portal><KSelect.Content class={kSelectContentClass}><KSelect.Listbox /></KSelect.Content></KSelect.Portal>
                                                    </KSelect>
                                                </div>
                                            </Show>
                                            {/* Priority */}
                                            <div>
                                                <TextField value={rule.priority.toString()} onChange={v => updateRuleField(index(), 'priority', v === '' ? 0 : parseInt(v))} type="number">
                                                    <TextField.Label class={`${formLabelClass} ${formLabelSmClass}`}>{t('accessControlPage.rules.labelPriority')}</TextField.Label>
                                                    <TextField.Input class={formInputSmClass} />
                                                </TextField>
                                            </div>
                                            {/* Actions */}
                                            <div class="self-end">
                                                <Button onClick={() => removeRule(index())} class="btn btn-danger btn-sm">{t('accessControlPage.rules.deleteRule')}</Button>
                                            </div>
                                        </div>
                                    </div>
                                }</For>
                            </div>
                        </div>

                        {/* Modal Actions */}
                        <div class="form-buttons flex justify-end gap-3 pt-6">
                            <Button onClick={handleCloseModal} class="btn btn-default">{t('common.cancel')}</Button>
                            <Button onClick={handleSavePolicy} class="btn btn-primary">{t('common.save')}</Button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    );
}

