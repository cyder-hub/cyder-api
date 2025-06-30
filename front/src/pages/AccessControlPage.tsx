import { createSignal, For, Show, createResource, Accessor, Setter, onMount, createMemo } from 'solid-js';
import type { Resource } from 'solid-js';
import { Button } from '../components/ui/Button';
import {
    DialogRoot,
    DialogContent,
    DialogHeader,
    DialogFooter,
    DialogTitle,
} from '../components/ui/Dialog';
import { Select } from '../components/ui/Select';
import { TextField, NumberField } from '../components/ui/Input';
import {
    TableRoot,
    TableHeader,
    TableBody,
    TableRow,
    TableColumnHeader,
    TableCell,
} from '../components/ui/Table';
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

    const ruleTypeOptions = createMemo(() => ['ALLOW', 'DENY'].map(o => ({ value: o, label: t(`accessControlPage.modal.option${o}`) })));
    const scopeOptions = createMemo(() => ['PROVIDER', 'MODEL'].map(o => ({ value: o, label: t(`accessControlPage.rules.scope${o}`) })));

    const [policies, { refetch: refetchPolicies }] = createResource<AccessControlPolicyFromAPI[]>(fetchPoliciesAPI, { initialValue: [] });
    const [providers] = createResource<ProviderDetail[]>(fetchProvidersWithModelsAPI, { initialValue: [] });

    const [showEditModal, setShowEditModal] = createSignal(false);
    const [editingPolicy, setEditingPolicy] = createSignal<AccessControlPolicyUI>(newPolicyTemplate());

    const defaultActionOptions = createMemo(() => ['ALLOW', 'DENY'].map(o => ({ value: o, label: t(`accessControlPage.modal.option${o}`) })));
    const selectedDefaultAction = createMemo(() => defaultActionOptions().find(o => o.value === (editingPolicy()?.default_action || 'ALLOW')));

    const providerOptions = createMemo(() => (providers() || []).map(p => ({ value: p.provider.id, label: p.provider.name })));

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
                <Button onClick={handleOpenAddModal} variant="primary">{t('accessControlPage.addPolicy')}</Button>
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
                    <TableRoot>
                        <TableHeader>
                            <TableRow>
                                <TableColumnHeader>{t('accessControlPage.table.name')}</TableColumnHeader>
                                <TableColumnHeader>{t('accessControlPage.table.defaultAction')}</TableColumnHeader>
                                <TableColumnHeader>{t('accessControlPage.table.description')}</TableColumnHeader>
                                <TableColumnHeader>{t('accessControlPage.table.rules')}</TableColumnHeader>
                                <TableColumnHeader>{t('accessControlPage.table.actions')}</TableColumnHeader>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            <For each={policies()}>{(policy) =>
                                <TableRow>
                                    <TableCell>{policy.name}</TableCell>
                                    <TableCell>{t(`accessControlPage.modal.option${policy.default_action}`)}</TableCell>
                                    <TableCell>{policy.description || '/'}</TableCell>
                                    <TableCell>{t('accessControlPage.table.rulesCount', { count: policy.rules?.length || 0 })}</TableCell>
                                    <TableCell class="space-x-2">
                                        <Button onClick={() => handleOpenEditModal(policy.id)} variant="primary" size="sm">{t('common.edit')}</Button>
                                        <Button onClick={() => handleDeletePolicy(policy.id, policy.name)} variant="destructive" size="sm">{t('common.delete')}</Button>
                                    </TableCell>
                                </TableRow>
                            }</For>
                        </TableBody>
                    </TableRoot>
                </div>
            </Show>

            {/* Edit/Add Modal */}
            <DialogRoot open={showEditModal()} onOpenChange={setShowEditModal}>
                <DialogContent class="max-w-4xl max-h-[90vh] flex flex-col">
                    <DialogHeader>
                        <DialogTitle>{editingPolicy()?.id ? t('accessControlPage.modal.titleEdit') : t('accessControlPage.modal.titleAdd')}</DialogTitle>
                    </DialogHeader>
                    <div class="space-y-4 overflow-y-auto">
                        {/* Policy Fields */}
                        <TextField
                            label={t('accessControlPage.modal.labelName')}
                            value={editingPolicy()?.name || ''}
                            onChange={(v) => setEditingPolicy(p => ({ ...p!, name: v }))}
                        />
                        <Select
                            label={t('accessControlPage.modal.labelDefaultAction')}
                            value={selectedDefaultAction()}
                            optionValue="value"
                            optionTextValue="label"
                            onChange={(v) => setEditingPolicy(p => ({ ...p!, default_action: v.value as 'ALLOW' | 'DENY' }))}
                            options={defaultActionOptions()}
                            placeholder={t('accessControlPage.modal.placeholderDefaultAction')}
                        />
                        <TextField
                            label={t('accessControlPage.modal.labelDescription')}
                            value={editingPolicy()?.description || ''}
                            onChange={(v) => setEditingPolicy(p => ({ ...p!, description: v }))}
                            textarea
                            rows={2}
                        />

                        <hr class="my-6" />

                        {/* Rules Section */}
                        <div class="space-y-3">
                            <div class="flex justify-between items-center">
                                <h4 class="text-lg font-medium">{t('accessControlPage.rules.title')}</h4>
                                <Button onClick={addRule} variant="secondary" size="sm">{t('accessControlPage.rules.addRule')}</Button>
                            </div>
                            <div class="max-h-80 overflow-y-auto border rounded p-2 space-y-2">
                                <Show when={editingPolicy()?.rules.length === 0}>
                                    <p class="text-sm text-gray-500 text-center py-2">{t('accessControlPage.rules.noRules')}</p>
                                </Show>
                                <For each={editingPolicy()?.rules}>{(rule, index) => {
                                    const modelOptions = createMemo(() => getModelsForProvider(rule.provider_id).map(m => ({ value: m.id, label: m.model_name })));
                                    const selectedModel = createMemo(() => modelOptions().find(m => m.value === rule.model_id));

                                    return (<div class="p-3 border rounded bg-gray-50 space-y-2">
                                        <div class="grid grid-cols-1 md:grid-cols-3 lg:grid-cols-6 gap-2 items-end">
                                            {/* Rule Type */}
                                            <div>
                                                <Select
                                                    value={ruleTypeOptions().find(o => o.value === rule.rule_type)}
                                                    onChange={(v) => updateRuleField(index(), 'rule_type', v.value)}
                                                    optionValue="value"
                                                    optionTextValue="label"
                                                    options={ruleTypeOptions()}
                                                    label={t('accessControlPage.rules.labelRuleType')}
                                                />
                                            </div>
                                            {/* Scope */}
                                            <div>
                                                <Select
                                                    value={scopeOptions().find(o => o.value === rule.scope)}
                                                    onChange={(v) => updateRuleField(index(), 'scope', v.value)}
                                                    optionValue="value"
                                                    optionTextValue="label"
                                                    options={scopeOptions()}
                                                    label={t('accessControlPage.rules.labelScope')}
                                                />
                                            </div>
                                            {/* Provider */}
                                            <div classList={{ "lg:col-span-2": rule.scope === 'PROVIDER' }}>
                                                <Select
                                                    value={providerOptions().find(p => p.value === rule.provider_id)}
                                                    optionValue="value"
                                                    optionTextValue="label"
                                                    onChange={(v) => updateRuleField(index(), 'provider_id', v ? v.value : null)}
                                                    options={providerOptions()}
                                                    placeholder={t('accessControlPage.rules.placeholderProvider')}
                                                    label={t('accessControlPage.rules.labelProvider')}
                                                />
                                            </div>
                                            {/* Model */}
                                            <Show when={rule.scope === 'MODEL'}>
                                                <div>
                                                    <Select
                                                        value={selectedModel()}
                                                        onChange={(v) => updateRuleField(index(), 'model_id', v ? v.value : null)}
                                                        optionValue="value"
                                                        optionTextValue="label"
                                                        options={modelOptions()}
                                                        placeholder={t('accessControlPage.rules.placeholderModel')}
                                                        disabled={!rule.provider_id}
                                                        label={t('accessControlPage.rules.labelModel')}
                                                    />
                                                </div>
                                            </Show>
                                            {/* Priority */}
                                            <div>
                                                <NumberField
                                                    label={t('accessControlPage.rules.labelPriority')}
                                                    value={rule.priority}
                                                    onChange={v => updateRuleField(index(), 'priority', isNaN(v) ? 0 : v)}
                                                    step={1}
                                                    formatOptions={{ maximumFractionDigits: 0 }}
                                                />
                                            </div>
                                            {/* Actions */}
                                            <div class="self-end">
                                                <Button onClick={() => removeRule(index())} variant="destructive" size="sm">{t('accessControlPage.rules.deleteRule')}</Button>
                                            </div>
                                        </div>
                                    </div>)
                                }}</For>
                            </div>
                        </div>
                    </div>
                    <DialogFooter class="pt-6">
                        <Button onClick={handleCloseModal} variant="secondary">{t('common.cancel')}</Button>
                        <Button onClick={handleSavePolicy} variant="primary">{t('common.save')}</Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>
        </div>
    );
}

