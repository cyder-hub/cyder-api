import { createResource, createSignal, createRoot } from 'solid-js';
import { request } from '@/services/api';
import { toastController } from '@/components/GlobalMessage';

export interface BillingPlan {
    id: number;
    name: string;
    description: string | null;
    currency: string;
    created_at: number;
    updated_at: number;
}

export interface PriceRule {
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

const fetchBillingPlansAPI = async (): Promise<BillingPlan[]> => {
    try {
        const response = await request('/ai/manager/api/price/plan/list');
        return response || [];
    } catch (error) {
        console.error("Failed to fetch billing plans", error);
        toastController.error("Failed to fetch billing plans");
        return [];
    }
};

const fetchPriceRulesAPI = async (planId: number | null): Promise<PriceRule[]> => {
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

function createPriceStore() {
    const [shouldFetchPlans, setShouldFetchPlans] = createSignal(false);

    const [plans, { refetch: refetchPlans }] = createResource<BillingPlan[]>(shouldFetchPlans, fetchBillingPlansAPI, { initialValue: [] });

    const [selectedPlanId, setSelectedPlanId] = createSignal<number | null>(null);

    const [rules, { refetch: refetchRules }] = createResource(selectedPlanId, fetchPriceRulesAPI);

    function loadBillingPlans() {
        setShouldFetchPlans(true);
    }

    return {
        billingPlans: plans,
        refetchBillingPlans: refetchPlans,
        selectedPlanId,
        setSelectedPlanId,
        priceRules: rules,
        refetchPriceRules: refetchRules,
        loadBillingPlans,
    };
}

const store = createRoot(createPriceStore);

export const billingPlans = store.billingPlans;
export const refetchBillingPlans = store.refetchBillingPlans;
export const selectedPlanId = store.selectedPlanId;
export const setSelectedPlanId = store.setSelectedPlanId;
export const priceRules = store.priceRules;
export const refetchPriceRules = store.refetchPriceRules;
export const loadBillingPlans = store.loadBillingPlans;
