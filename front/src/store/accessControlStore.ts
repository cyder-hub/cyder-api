import { createResource, createSignal, createRoot } from 'solid-js';
import { request } from '../services/api';

interface AccessControlPolicyBase {
    name: string;
    default_action: 'ALLOW' | 'DENY';
    description: string | null;
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

export const fetchPoliciesAPI = async (): Promise<AccessControlPolicyFromAPI[]> => {
    try {
        const response = await request("/ai/manager/api/access_control/list");
        return response || [];
    } catch (error) {
        console.error("Failed to fetch policies:", error);
        return [];
    }
};

function createAccessControlStore() {
    const [shouldFetch, setShouldFetch] = createSignal(false);

    // Eagerly fetch policies on demand
    const [policies, { refetch: refetchPolicies }] = createResource<AccessControlPolicyFromAPI[]>(shouldFetch, fetchPoliciesAPI, { initialValue: [] });

    function loadPolicies() {
        setShouldFetch(true);
    }
    return { policies, refetchPolicies, loadPolicies };
}

const { policies, refetchPolicies, loadPolicies } = createRoot(createAccessControlStore);

export { policies, refetchPolicies, loadPolicies };
