// --- Shared Type Definitions for Providers and related entities ---

export interface ModelItem {
    id: number; // Server-side ID
    model_name: string;
    real_model_name: string;
}

export interface ProviderApiKeyItem {
    id: number; // Server-side ID, always present for existing keys from backend
    api_key: string;
    description: string | null; // To match Option<String> from backend
    // Optional: Add other fields if needed from backend ProviderApiKey struct
    // e.g., is_enabled?: boolean; created_at?: number; updated_at?: number;
}

export type CustomFieldType = 'unset' | 'text' | 'integer' | 'float' | 'boolean';

export interface CustomFieldItem {
    // id?: number; // Not strictly needed for commit
    field_name: string;
    field_value: string;
    description: string;
    field_type: CustomFieldType;
}

export interface ProviderBase {
    id: number;
    provider_key: string;
    name: string;
    endpoint: string;
    use_proxy: boolean;
    provider_type: string;
}

export interface ModelDetail {
    model: ModelItem;
    custom_fields: CustomFieldItem[];
}

export interface ProviderListItem {
    provider: ProviderBase;
    models: ModelDetail[];
    provider_keys: ProviderApiKeyItem[];
    custom_fields: CustomFieldItem[];
}

export interface ApiKeyItem {
    id: number;
    name: string;
    api_key: string; // The actual key, may be masked in list view
    description: string;
    is_enabled: boolean;
    created_at: number; // Milliseconds timestamp
    updated_at: number; // Milliseconds timestamp
    // For display
    created_at_formatted?: string;
    updated_at_formatted?: string;
}
