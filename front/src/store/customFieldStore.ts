import { createResource, createSignal, createRoot } from 'solid-js';
import { request } from '@/services/api';

export interface CustomFieldDefinition {
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

const fetchCustomFieldsAPI = async (): Promise<CustomFieldDefinition[]> => {
    try {
        const response = await request("/ai/manager/api/custom_field_definition/list?page_size=1000");
        return response.list || [];
    } catch (error) {
        console.error("Failed to fetch custom fields:", error);
        throw error;
    }
};

function createCustomFieldStore() {
    const [shouldFetch, setShouldFetch] = createSignal(false);

    const [resource, { refetch }] = createResource<CustomFieldDefinition[]>(shouldFetch, fetchCustomFieldsAPI, { initialValue: [] });

    function loadCustomFields() {
        setShouldFetch(true);
    }

    return {
        customFields: resource,
        refetchCustomFields: refetch,
        loadCustomFields,
    };
}

const store = createRoot(createCustomFieldStore);

export const customFields = store.customFields;
export const refetchCustomFields = store.refetchCustomFields;
export const loadCustomFields = store.loadCustomFields;
