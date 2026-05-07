// ========== Shared JSON Types ==========
export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];
export interface JsonObject {
  [key: string]: JsonValue;
}

// ========== Paginated Response ==========
export interface PaginatedResponse<T> {
  list: T[];
  total?: number;
}

// ========== Runtime State Backend Types ==========
export type RuntimeStateBackendName = "memory" | "redis" | string;
export type DeploymentMode = "single_instance" | "multi_instance" | string;

export interface RuntimeStateBackendStatus {
  deployment_mode: DeploymentMode;
  catalog_cache_backend: RuntimeStateBackendName;
  catalog_cache_configured_backend: RuntimeStateBackendName;
  catalog_cache_effective_backend: RuntimeStateBackendName;
  catalog_cache_fallback_reason: string | null;
  runtime_configured_backend: RuntimeStateBackendName;
  runtime_effective_backend: RuntimeStateBackendName;
  runtime_shared: boolean;
  runtime_degraded: boolean;
  fallback_reason: string | null;
  last_error: string | null;
  last_checked_at: number;
}
