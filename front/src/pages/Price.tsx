import { createSignal, For, Show, createResource } from 'solid-js';
import { createStore } from 'solid-js/store';
import { useI18n } from '../i18n';
import { request } from '../services/api';
import { toastController } from '../components/GlobalMessage';
import { Button } from '@kobalte/core/button';
import { Dialog } from '@kobalte/core/dialog';
import { TextField } from '@kobalte/core/text-field';
import { Checkbox } from '@kobalte/core/checkbox';
import { Select } from '@kobalte/core/select';
import { NumberField } from '@kobalte/core/number-field';

// Interfaces based on backend
interface BillingPlan {
    id: number;
    name: string;
    description: string | null;
    currency: string;
    created_at: number;
    updated_at: number;
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

// Editing types
type EditingBillingPlan = Omit<BillingPlan, 'created_at' | 'updated_at'>;
type EditingPriceRule = Omit<PriceRule, 'created_at' | 'updated_at'>;

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

const fetchPriceRules = async (planId: number): Promise<PriceRule[]> => {
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

const formatTimestamp = (ms: number | undefined | null): string => {
    if (!ms) return '';
    const date = new Date(ms);
    return date.toLocaleString();
};

const toDateTimeLocal = (ms: number | null | undefined) => {
    if (!ms) return '';
    // Adjust for timezone offset to display correctly in local time input
    const date = new Date(ms);
    const timezoneOffset = date.getTimezoneOffset() * 60000;
    return new Date(date.getTime() - timezoneOffset).toISOString().slice(0, 16);
};

const fromDateTimeLocal = (str: string) => {
    return str ? new Date(str).getTime() : null;
};

export default function PricePage() {
    const [t] = useI18n();

    const [billingPlans, { refetch: refetchBillingPlans }] = createResource(fetchBillingPlans);
    const [selectedPlanId, setSelectedPlanId] = createSignal<number | null>(null);
    const [priceRules, { refetch: refetchPriceRules }] = createResource(selectedPlanId, fetchPriceRules);

    const selectedPlan = () => billingPlans()?.find(p => p.id === selectedPlanId());

    const [isPlanModalOpen, setPlanModalOpen] = createSignal(false);
    const [editingPlan, setEditingPlan] = createStore<Partial<EditingBillingPlan>>({});

    const [isRuleModalOpen, setRuleModalOpen] = createSignal(false);
    const [editingRule, setEditingRule] = createStore<Partial<EditingPriceRule>>({});

    const handleSelectPlan = (planId: number) => {
        setSelectedPlanId(planId);
    };

    const openNewPlanModal = () => {
        setEditingPlan({ id: null, name: '', description: '', currency: 'USD' });
        setPlanModalOpen(true);
    };

    const openEditPlanModal = (plan: BillingPlan) => {
        setEditingPlan(plan);
        setPlanModalOpen(true);
    };

    const handleSavePlan = async () => {
        const plan = editingPlan;
        if (!plan.name) {
            toastController.warn(t('pricePage.alert.planNameRequired'));
            return;
        }

        const payload = {
            name: plan.name,
            description: plan.description,
            currency: plan.currency,
        };

        try {
            if (plan.id) {
                await request(`/ai/manager/api/price/plan/${plan.id}`, {
                    method: 'PUT',
                    body: JSON.stringify(payload),
                });
            } else {
                await request('/ai/manager/api/price/plan', {
                    method: 'POST',
                    body: JSON.stringify(payload),
                });
            }
            toastController.success(t('pricePage.alert.planSaveSuccess'));
            setPlanModalOpen(false);
            refetchBillingPlans();
        } catch (error) {
            toastController.error(t('pricePage.alert.planSaveFailed', { error: (error as Error).message }));
        }
    };

    const handleDeletePlan = async (planId: number, planName: string) => {
        if (confirm(t('pricePage.confirmDeletePlan', { name: planName }))) {
            try {
                await request(`/ai/manager/api/price/plan/${planId}`, { method: 'DELETE' });
                toastController.success(t('pricePage.alert.planDeleteSuccess'));
                refetchBillingPlans();
                if (selectedPlanId() === planId) {
                    setSelectedPlanId(null);
                }
            } catch (error) {
                toastController.error(t('pricePage.alert.planDeleteFailed', { error: (error as Error).message }));
            }
        }
    };

    const openNewRuleModal = () => {
        if (!selectedPlanId()) {
            toastController.warn(t('pricePage.alert.selectPlanFirst'));
            return;
        }
        setEditingRule({
            id: null,
            plan_id: selectedPlanId()!,
            description: '',
            is_enabled: true,
            effective_from: Date.now(),
            effective_until: null,
            usage_type: 'COMPLETION',
            media_type: null,
            price_in_micro_units: 0,
        });
        setRuleModalOpen(true);
    };

    const openEditRuleModal = (rule: PriceRule) => {
        const modifiedRule = { ...rule, price_in_micro_units: rule.price_in_micro_units / 1000 };
        setEditingRule(modifiedRule);
        setRuleModalOpen(true);
    };

    const handleSaveRule = async () => {
        const rule = editingRule;
        const price = Number(rule.price_in_micro_units);
        const payload = {
            ...rule,
            media_type: rule.media_type || null,
            price_in_micro_units: isNaN(price) ? 0 : Math.round(price * 1000),
        };

        try {
            if (rule.id) {
                await request(`/ai/manager/api/price/rule/${rule.id}`, {
                    method: 'PUT',
                    body: JSON.stringify(payload),
                });
            } else {
                await request('/ai/manager/api/price/rule', {
                    method: 'POST',
                    body: JSON.stringify(payload),
                });
            }
            toastController.success(t('pricePage.alert.ruleSaveSuccess'));
            setRuleModalOpen(false);
            refetchPriceRules();
        } catch (error) {
            toastController.error(t('pricePage.alert.ruleSaveFailed', { error: (error as Error).message }));
        }
    };

    const handleDeleteRule = async (ruleId: number) => {
        if (confirm(t('pricePage.confirmDeleteRule'))) {
            try {
                await request(`/ai/manager/api/price/rule/${ruleId}`, { method: 'DELETE' });
                toastController.success(t('pricePage.alert.ruleDeleteSuccess'));
                refetchPriceRules();
            } catch (error) {
                toastController.error(t('pricePage.alert.ruleDeleteFailed', { error: (error as Error).message }));
            }
        }
    };

    const USAGE_TYPES = ['PROMPT', 'COMPLETION', 'INVOCATION'];
    const MEDIA_TYPES = ['', 'IMAGE', 'AUDIO', 'VIDEO', 'CACHE_TEXT', 'CACHE_AUDIO', 'CACHE_VIDEO'];

    return (
        <div class="p-4 space-y-6 bg-white rounded-lg shadow-xl max-w-6xl mx-auto my-8">
            <h1 class="text-2xl font-semibold text-gray-800">{t('pricePage.title')}</h1>

            {/* Billing Plans Section */}
            <div class="section">
                <div class="flex justify-between items-center mb-4">
                    <h2 class="section-title">{t('pricePage.plans.title')}</h2>
                    <Button class="btn btn-primary" onClick={openNewPlanModal}>{t('pricePage.plans.add')}</Button>
                </div>
                <Show when={!billingPlans.loading} fallback={<p>{t('pricePage.plans.loading')}</p>}>
                    <table class="data-table min-w-full">
                    <thead class="bg-gray-100">
                        <tr>
                            <th class="px-4 py-2 text-left">{t('pricePage.plans.table.name')}</th>
                            <th class="px-4 py-2 text-left">{t('pricePage.plans.table.description')}</th>
                            <th class="px-4 py-2 text-left">{t('pricePage.plans.modal.currency')}</th>
                            <th class="px-4 py-2 text-left">{t('pricePage.plans.table.actions')}</th>
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-gray-200">
                        <For each={billingPlans()}>
                            {(plan) => (
                                <tr
                                    class="cursor-pointer hover:bg-gray-100"
                                    classList={{ 'bg-blue-100 font-semibold': selectedPlanId() === plan.id }}
                                    onClick={() => handleSelectPlan(plan.id)}
                                >
                                    <td class="px-4 py-2">{plan.name}</td>
                                    <td class="px-4 py-2">{plan.description}</td>
                                    <td class="px-4 py-2">{plan.currency}</td>
                                    <td class="px-4 py-2">
                                        <Button class="btn btn-secondary btn-sm" onClick={(e) => { e.stopPropagation(); openEditPlanModal(plan); }}>{t('common.edit')}</Button>
                                        <Button class="btn btn-danger btn-sm ml-2" onClick={(e) => { e.stopPropagation(); handleDeletePlan(plan.id, plan.name); }}>{t('common.delete')}</Button>
                                    </td>
                                </tr>
                            )}
                        </For>
                    </tbody>
                </table>
                </Show>
            </div>

            {/* Price Rules Section */}
            <Show when={selectedPlanId()}>
                <div class="section">
                    <div class="flex justify-between items-center mb-4">
                        <h2 class="section-title">{t('pricePage.rules.title')}</h2>
                        <Button class="btn btn-primary" onClick={openNewRuleModal}>{t('pricePage.rules.add')}</Button>
                    </div>
                    <Show when={priceRules.loading}><p>{t('pricePage.rules.loading')}</p></Show>
                    <table class="data-table min-w-full">
                        <thead class="bg-gray-100">
                            <tr>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.description')}</th>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.enabled')}</th>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.usageType')}</th>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.mediaType')}</th>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.price')}</th>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.effectiveFrom')}</th>
                                <th class="px-4 py-2 text-left">{t('pricePage.rules.table.actions')}</th>
                            </tr>
                        </thead>
                        <tbody class="divide-y divide-gray-200">
                            <For each={priceRules()}>
                                {(rule) => (
                                    <tr>
                                        <td class="px-4 py-2">{rule.description}</td>
                                        <td class="px-4 py-2">{rule.is_enabled ? t('common.yes') : t('common.no')}</td>
                                        <td class="px-4 py-2">{rule.usage_type}</td>
                                        <td class="px-4 py-2">{rule.media_type}</td>
                                        <td class="px-4 py-2 text-right">{rule.price_in_micro_units / 1000} {selectedPlan()?.currency}</td>
                                        <td class="px-4 py-2">{formatTimestamp(rule.effective_from)}</td>
                                        <td class="px-4 py-2">
                                            <Button class="btn btn-secondary btn-sm" onClick={() => openEditRuleModal(rule)}>{t('common.edit')}</Button>
                                            <Button class="btn btn-danger btn-sm ml-2" onClick={() => handleDeleteRule(rule.id)}>{t('common.delete')}</Button>
                                        </td>
                                    </tr>
                                )}
                            </For>
                        </tbody>
                    </table>
                </div>
            </Show>

            {/* Billing Plan Modal */}
            <Dialog open={isPlanModalOpen()} onOpenChange={setPlanModalOpen}>
                <Dialog.Portal>
                    <Dialog.Overlay class="fixed inset-0 bg-black bg-opacity-50" />
                    <div class="fixed inset-0 flex items-center justify-center p-4">
                        <Dialog.Content class="bg-white p-6 rounded-lg shadow-lg w-full max-w-md">
                            <Dialog.Title class="text-xl font-bold mb-4">{editingPlan.id ? t('pricePage.plans.modal.titleEdit') : t('pricePage.plans.modal.titleAdd')}</Dialog.Title>
                            <div class="space-y-4">
                                <TextField value={editingPlan.name ?? ''} onChange={(v) => setEditingPlan('name', v)}>
                                    <TextField.Label class="form-label">{t('pricePage.plans.modal.name')}</TextField.Label>
                                    <TextField.Input class="form-input" />
                                </TextField>
                                <TextField value={editingPlan.description ?? ''} onChange={(v) => setEditingPlan('description', v)}>
                                    <TextField.Label class="form-label">{t('pricePage.plans.modal.description')}</TextField.Label>
                                    <TextField.Input class="form-input" />
                                </TextField>
                                <Select
                                    value={editingPlan.currency}
                                    onChange={(v) => setEditingPlan('currency', v)}
                                    options={['USD', 'CNY']}
                                    itemComponent={props => (
                                        <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                                            <Select.ItemLabel>{props.item.rawValue}</Select.ItemLabel>
                                        </Select.Item>
                                    )}
                                >
                                    <Select.Label class="form-label">{t('pricePage.plans.modal.currency')}</Select.Label>
                                    <Select.Trigger class="form-select w-full">
                                        <Select.Value>{state => state.selectedOption()}</Select.Value>
                                    </Select.Trigger>
                                    <Select.Portal>
                                        <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                                            <Select.Listbox class="max-h-60 overflow-y-auto py-1" />
                                        </Select.Content>
                                    </Select.Portal>
                                </Select>
                            </div>
                            <div class="mt-6 flex justify-end space-x-2">
                                <Button class="btn btn-secondary" onClick={() => setPlanModalOpen(false)}>{t('common.cancel')}</Button>
                                <Button class="btn btn-primary" onClick={handleSavePlan}>{t('common.save')}</Button>
                            </div>
                        </Dialog.Content>
                    </div>
                </Dialog.Portal>
            </Dialog>

            {/* Price Rule Modal */}
            <Dialog open={isRuleModalOpen()} onOpenChange={setRuleModalOpen}>
                <Dialog.Portal>
                    <Dialog.Overlay class="fixed inset-0 bg-black bg-opacity-50 z-50" />
                    <div class="fixed inset-0 flex items-center justify-center p-4 z-50">
                        <Dialog.Content class="bg-white p-6 rounded-lg shadow-lg w-full max-w-2xl max-h-[90vh] overflow-y-auto">
                            <Dialog.Title class="text-xl font-bold mb-4">{editingRule.id ? t('pricePage.rules.modal.titleEdit') : t('pricePage.rules.modal.titleAdd')}</Dialog.Title>
                            <div class="space-y-4">
                                <TextField value={editingRule.description ?? ''} onChange={(v) => setEditingRule('description', v)}>
                                    <TextField.Label class="form-label">{t('pricePage.rules.modal.description')}</TextField.Label>
                                    <TextField.Input class="form-input" />
                                </TextField>
                                <Checkbox checked={editingRule.is_enabled ?? false} onChange={(v) => setEditingRule('is_enabled', v)}>
                                    <Checkbox.Input class="form-checkbox" />
                                    <Checkbox.Label class="form-label ml-2">{t('pricePage.rules.modal.enabled')}</Checkbox.Label>
                                </Checkbox>
                                <div class="grid grid-cols-2 gap-4">
                                    <Select
                                        value={editingRule.usage_type}
                                        onChange={(v) => setEditingRule('usage_type', v)}
                                        options={USAGE_TYPES}
                                        itemComponent={props => (
                                            <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                                                <Select.ItemLabel>{props.item.rawValue}</Select.ItemLabel>
                                            </Select.Item>
                                        )}
                                    >
                                        <Select.Label class="form-label">{t('pricePage.rules.modal.usageType')}</Select.Label>
                                        <Select.Trigger class="form-select w-full">
                                            <Select.Value>{state => state.selectedOption()}</Select.Value>
                                        </Select.Trigger>
                                        <Select.Portal>
                                            <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                                                <Select.Listbox class="max-h-60 overflow-y-auto py-1" />
                                            </Select.Content>
                                        </Select.Portal>
                                    </Select>
                                    <Select
                                        value={editingRule.media_type ?? ''}
                                        onChange={(v) => setEditingRule('media_type', v)}
                                        options={MEDIA_TYPES}
                                        itemComponent={props => (
                                            <Select.Item item={props.item} class="flex justify-between items-center px-3 py-1.5 text-sm text-gray-700 ui-highlighted:bg-blue-100 ui-highlighted:text-blue-700 ui-selected:font-semibold outline-none cursor-default">
                                                <Select.ItemLabel>{props.item.rawValue || t('pricePage.rules.modal.mediaTypeDefault')}</Select.ItemLabel>
                                            </Select.Item>
                                        )}
                                    >
                                        <Select.Label class="form-label">{t('pricePage.rules.modal.mediaType')}</Select.Label>
                                        <Select.Trigger class="form-select w-full">
                                            <Select.Value>{state => state.selectedOption() || t('pricePage.rules.modal.mediaTypeDefault')}</Select.Value>
                                        </Select.Trigger>
                                        <Select.Portal>
                                            <Select.Content class="bg-white border border-gray-300 rounded shadow-lg mt-1 z-50">
                                                <Select.Listbox class="max-h-60 overflow-y-auto py-1" />
                                            </Select.Content>
                                        </Select.Portal>
                                    </Select>
                                </div>
                                <NumberField value={editingRule.price_in_micro_units ?? 0} onChange={(v) => setEditingRule('price_in_micro_units', v)}>
                                    <NumberField.Label class="form-label">{t('pricePage.rules.modal.price')}</NumberField.Label>
                                    <NumberField.Input class="form-input" />
                                </NumberField>
                                <div class="grid grid-cols-2 gap-4">
                                    <TextField
                                        type="datetime-local"
                                        value={toDateTimeLocal(editingRule.effective_from)}
                                        onChange={(v) => setEditingRule('effective_from', fromDateTimeLocal(v))}
                                    >
                                        <TextField.Label class="form-label">{t('pricePage.rules.modal.effectiveFrom')}</TextField.Label>
                                        <TextField.Input class="form-input" />
                                    </TextField>
                                    <TextField
                                        type="datetime-local"
                                        value={toDateTimeLocal(editingRule.effective_until)}
                                        onChange={(v) => setEditingRule('effective_until', fromDateTimeLocal(v))}
                                    >
                                        <TextField.Label class="form-label">{t('pricePage.rules.modal.effectiveUntil')}</TextField.Label>
                                        <TextField.Input class="form-input" />
                                    </TextField>
                                </div>
                            </div>
                            <div class="mt-6 flex justify-end space-x-2">
                                <Button class="btn btn-secondary" onClick={() => setRuleModalOpen(false)}>{t('common.cancel')}</Button>
                                <Button class="btn btn-primary" onClick={handleSaveRule}>{t('common.save')}</Button>
                            </div>
                        </Dialog.Content>
                    </div>
                </Dialog.Portal>
            </Dialog>

            <style jsx global>{`
                .section {
                    margin-bottom: 1.25rem; /* 20px */
                    padding: 1rem; /* 16px */
                    border: 1px solid #e5e7eb; /* gray-200 */
                    border-radius: 0.375rem; /* rounded-md */
                }
                .section-title {
                    font-size: 1.25rem; /* text-xl */
                    font-weight: 600; /* font-semibold */
                }
                .form-label { display: block; margin-bottom: 0.25rem; font-weight: 500; color: #374151; /* gray-700 */ }
                .form-input, .form-select {
                    width: 100%;
                    padding: 0.5rem 0.75rem;
                    border: 1px solid #d1d5db; /* gray-300 */
                    border-radius: 0.375rem; /* rounded-md */
                }
                .form-checkbox {
                    border-radius: 0.25rem;
                    border-color: #d1d5db; /* gray-300 */
                }
                .btn {
                    padding: 0.5rem 1rem;
                    border-radius: 0.375rem;
                    font-weight: 500;
                }
                .btn-sm { padding: 0.25rem 0.75rem; font-size: 0.875rem; }
                .btn-primary { background-color: #2563eb; color: white; }
                .btn-primary:hover { background-color: #1d4ed8; }
                .btn-secondary { background-color: #6b7280; color: white; }
                .btn-secondary:hover { background-color: #4b5563; }
                .btn-danger { background-color: #dc2626; color: white; }
                .btn-danger:hover { background-color: #b91c1c; }
                .data-table th, .data-table td {
                    padding: 0.75rem;
                    vertical-align: middle;
                }
            `}</style>
        </div>
    );
}
