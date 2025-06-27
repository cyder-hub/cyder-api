import { createSignal, For, Show, createResource, createMemo } from 'solid-js';
import { createStore } from 'solid-js/store';
import { useI18n } from '../i18n';
import { request } from '../services/api';
import { toastController } from '../components/GlobalMessage';
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
    const MEDIA_TYPES = ['', 'IMAGE', 'AUDIO', 'VIDEO', 'CACHE_TEXT', 'CACHE_AUDIO', 'CACHE_VIDEO'].map(mt => ({ value: mt, label: mt || t('pricePage.rules.modal.mediaTypeDefault') }));

    const selectedMediaType = createMemo(() => MEDIA_TYPES.find(o => o.value === (editingRule.media_type ?? '')));

    return (
        <div class="p-4 space-y-6 bg-white rounded-lg shadow-xl max-w-6xl mx-auto my-8">
            <h1 class="text-2xl font-semibold text-gray-800">{t('pricePage.title')}</h1>

            {/* Billing Plans Section */}
            <div class="section">
                <div class="flex justify-between items-center mb-4">
                    <h2 class="section-title">{t('pricePage.plans.title')}</h2>
                    <Button variant="primary" onClick={openNewPlanModal}>{t('pricePage.plans.add')}</Button>
                </div>
                <Show when={!billingPlans.loading} fallback={<p>{t('pricePage.plans.loading')}</p>}>
                    <div class="shadow-md rounded-lg border border-gray-200 overflow-hidden">
                        <TableRoot>
                            <TableHeader>
                                <TableRow>
                                    <TableColumnHeader>{t('pricePage.plans.table.name')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.plans.table.description')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.plans.modal.currency')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.plans.table.actions')}</TableColumnHeader>
                                </TableRow>
                            </TableHeader>
                            <TableBody>
                                <For each={billingPlans()}>
                                    {(plan) => (
                                        <TableRow
                                            class="cursor-pointer"
                                            classList={{ 'bg-blue-100 font-semibold': selectedPlanId() === plan.id }}
                                            onClick={() => handleSelectPlan(plan.id)}
                                        >
                                            <TableCell>{plan.name}</TableCell>
                                            <TableCell>{plan.description}</TableCell>
                                            <TableCell>{plan.currency}</TableCell>
                                            <TableCell>
                                                <Button variant="secondary" size="sm" onClick={(e) => { e.stopPropagation(); openEditPlanModal(plan); }}>{t('common.edit')}</Button>
                                                <Button variant="destructive" size="sm" class="ml-2" onClick={(e) => { e.stopPropagation(); handleDeletePlan(plan.id, plan.name); }}>{t('common.delete')}</Button>
                                            </TableCell>
                                        </TableRow>
                                    )}
                                </For>
                            </TableBody>
                        </TableRoot>
                    </div>
                </Show>
            </div>

            {/* Price Rules Section */}
            <Show when={selectedPlanId()}>
                <div class="section">
                    <div class="flex justify-between items-center mb-4">
                        <h2 class="section-title">{t('pricePage.rules.title')}</h2>
                        <Button variant="primary" onClick={openNewRuleModal}>{t('pricePage.rules.add')}</Button>
                    </div>
                    <Show when={priceRules.loading}><p>{t('pricePage.rules.loading')}</p></Show>
                    <div class="shadow-md rounded-lg border border-gray-200 overflow-hidden">
                        <TableRoot>
                            <TableHeader>
                                <TableRow>
                                    <TableColumnHeader>{t('pricePage.rules.table.description')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.rules.table.enabled')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.rules.table.usageType')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.rules.table.mediaType')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.rules.table.price')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.rules.table.effectiveFrom')}</TableColumnHeader>
                                    <TableColumnHeader>{t('pricePage.rules.table.actions')}</TableColumnHeader>
                                </TableRow>
                            </TableHeader>
                            <TableBody>
                                <For each={priceRules()}>
                                    {(rule) => (
                                        <TableRow>
                                            <TableCell>{rule.description}</TableCell>
                                            <TableCell>{rule.is_enabled ? t('common.yes') : t('common.no')}</TableCell>
                                            <TableCell>{rule.usage_type}</TableCell>
                                            <TableCell>{rule.media_type}</TableCell>
                                            <TableCell class="text-right">{rule.price_in_micro_units / 1000} {selectedPlan()?.currency}</TableCell>
                                            <TableCell>{formatTimestamp(rule.effective_from)}</TableCell>
                                            <TableCell>
                                                <Button variant="secondary" size="sm" onClick={() => openEditRuleModal(rule)}>{t('common.edit')}</Button>
                                                <Button variant="destructive" size="sm" class="ml-2" onClick={() => handleDeleteRule(rule.id)}>{t('common.delete')}</Button>
                                            </TableCell>
                                        </TableRow>
                                    )}
                                </For>
                            </TableBody>
                        </TableRoot>
                    </div>
                </div>
            </Show>

            {/* Billing Plan Modal */}
            <DialogRoot open={isPlanModalOpen()} onOpenChange={setPlanModalOpen}>
                <DialogContent class="max-w-md">
                    <DialogHeader>
                        <DialogTitle>{editingPlan.id ? t('pricePage.plans.modal.titleEdit') : t('pricePage.plans.modal.titleAdd')}</DialogTitle>
                    </DialogHeader>
                    <div class="space-y-4">
                        <TextField
                            label={t('pricePage.plans.modal.name')}
                            value={editingPlan.name ?? ''}
                            onChange={(v) => setEditingPlan('name', v)}
                        />
                        <TextField
                            label={t('pricePage.plans.modal.description')}
                            value={editingPlan.description ?? ''}
                            onChange={(v) => setEditingPlan('description', v)}
                        />
                        <Select
                            value={editingPlan.currency}
                            onChange={(v) => setEditingPlan('currency', v)}
                            options={['USD', 'CNY']}
                            label={t('pricePage.plans.modal.currency')}
                        />
                    </div>
                    <DialogFooter class="mt-6">
                        <Button variant="secondary" onClick={() => setPlanModalOpen(false)}>{t('common.cancel')}</Button>
                        <Button variant="primary" onClick={handleSavePlan}>{t('common.save')}</Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>

            {/* Price Rule Modal */}
            <DialogRoot open={isRuleModalOpen()} onOpenChange={setRuleModalOpen}>
                <DialogContent class="max-w-2xl max-h-[90vh] overflow-y-auto">
                    <DialogHeader>
                        <DialogTitle>{editingRule.id ? t('pricePage.rules.modal.titleEdit') : t('pricePage.rules.modal.titleAdd')}</DialogTitle>
                    </DialogHeader>
                    <div class="space-y-4">
                        <TextField
                            label={t('pricePage.rules.modal.description')}
                            value={editingRule.description ?? ''}
                            onChange={(v) => setEditingRule('description', v)}
                        />
                        <div class="flex items-center space-x-2">
                            <input
                                type="checkbox"
                                id="is_enabled_rule_checkbox"
                                class="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
                                checked={editingRule.is_enabled ?? false}
                                onChange={(e) => setEditingRule('is_enabled', e.currentTarget.checked)}
                            />
                            <label for="is_enabled_rule_checkbox" class="text-sm font-medium leading-none">{t('pricePage.rules.modal.enabled')}</label>
                        </div>
                        <div class="grid grid-cols-2 gap-4">
                            <Select
                                value={editingRule.usage_type}
                                onChange={(v) => setEditingRule('usage_type', v)}
                                options={USAGE_TYPES}
                                label={t('pricePage.rules.modal.usageType')}
                            />
                            <Select
                                value={selectedMediaType()}
                                optionValue="value"
                                optionTextValue="label"
                                onChange={(v) => setEditingRule('media_type', v ? v.value : null)}
                                options={MEDIA_TYPES}
                                label={t('pricePage.rules.modal.mediaType')}
                            />
                        </div>
                        <NumberField
                            label={t('pricePage.rules.modal.price')}
                            value={editingRule.price_in_micro_units ?? 0}
                            onChange={(v) => setEditingRule('price_in_micro_units', v)}
                        />
                        <div class="grid grid-cols-2 gap-4">
                            <TextField
                                type="datetime-local"
                                label={t('pricePage.rules.modal.effectiveFrom')}
                                value={toDateTimeLocal(editingRule.effective_from)}
                                onChange={(v) => setEditingRule('effective_from', fromDateTimeLocal(v))}
                            />
                            <TextField
                                type="datetime-local"
                                label={t('pricePage.rules.modal.effectiveUntil')}
                                value={toDateTimeLocal(editingRule.effective_until)}
                                onChange={(v) => setEditingRule('effective_until', fromDateTimeLocal(v))}
                            />
                        </div>
                    </div>
                    <DialogFooter class="mt-6">
                        <Button variant="secondary" onClick={() => setRuleModalOpen(false)}>{t('common.cancel')}</Button>
                        <Button variant="primary" onClick={handleSaveRule}>{t('common.save')}</Button>
                    </DialogFooter>
                </DialogContent>
            </DialogRoot>

        </div>
    );
}
