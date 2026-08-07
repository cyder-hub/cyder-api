import type {
  ProviderApiKeySummary,
  ProviderBootstrapPayload,
  ProviderBootstrapResponse,
  ModelItem,
  ProviderUpdatePayload,
} from "@/services/types";
import type {
  EditingProviderData,
  LocalEditableModelItem,
  LocalProviderApiKeyItem,
} from "../types";

export interface ProviderBootstrapPreviewState {
  profile_type: string;
  endpoint: string;
  provider_name?: string;
  provider_key?: string;
  name?: string;
  key?: string;
  model_name?: string;
}

export interface ProviderBootstrapFormState extends ProviderBootstrapPreviewState {
  api_key: string;
  model_name: string;
  api_key_description: string;
  use_proxy: boolean;
  provider_name: string;
  provider_key: string;
  real_model_name?: string | null;
}

function getPreferredSource(editingData?: Partial<EditingProviderData> | null) {
  return (
    editingData?.upstream_sources?.find((source) => source.is_default) ??
    editingData?.upstream_sources?.find((source) => source.is_enabled) ??
    editingData?.upstream_sources?.[0]
  );
}

export function createProviderBootstrapFormState(
  editingData?: Partial<EditingProviderData> | null,
): ProviderBootstrapFormState {
  const source = getPreferredSource(editingData);
  return {
    profile_type: trimText(source?.profile_type) || "OPENAI",
    endpoint: trimText(source?.endpoint),
    api_key: "",
    model_name: "",
    api_key_description: "",
    use_proxy: source?.use_proxy ?? false,
    provider_name: trimText(editingData?.name),
    provider_key: trimText(editingData?.provider_key),
  };
}

export function syncProviderBootstrapFormState(
  form: ProviderBootstrapFormState,
  editingData?: Partial<EditingProviderData> | null,
): ProviderBootstrapFormState {
  if (!editingData) {
    return form;
  }

  const source = getPreferredSource(editingData);
  form.profile_type = trimText(source?.profile_type) || "OPENAI";
  form.endpoint = trimText(source?.endpoint);
  form.use_proxy = source?.use_proxy ?? false;
  form.provider_name = trimText(editingData.name);
  form.provider_key = trimText(editingData.provider_key);

  return form;
}

function trimText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function titleize(value: unknown): string {
  const text = trimText(value);
  if (!text) return "";

  return text
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .toLowerCase()
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function mapCreatedModel(
  model:
    | Partial<
        Pick<
          ModelItem,
          | "id"
          | "model_name"
          | "real_model_name"
          | "supports_streaming"
          | "supports_tools"
          | "supports_reasoning"
          | "supports_image_input"
          | "supports_embeddings"
          | "supports_rerank"
          | "is_enabled"
        >
      >
    | null
    | undefined,
): LocalEditableModelItem {
  return {
    id: model?.id ?? null,
    model_name: model?.model_name ?? "",
    real_model_name: model?.real_model_name ?? null,
    supports_streaming: model?.supports_streaming ?? true,
    supports_tools: model?.supports_tools ?? true,
    supports_reasoning: model?.supports_reasoning ?? true,
    supports_image_input: model?.supports_image_input ?? true,
    supports_embeddings: model?.supports_embeddings ?? true,
    supports_rerank: model?.supports_rerank ?? true,
    is_enabled: model?.is_enabled ?? true,
    isEditing: false,
    checkStatus: "unchecked",
  };
}

export function mapProviderApiKeySummary(
  key: ProviderApiKeySummary | null | undefined,
): LocalProviderApiKeyItem | null {
  if (!key) return null;
  return {
    id: key.id,
    provider_id: key.provider_id,
    description: key.description ?? null,
    key_prefix: key.key_prefix,
    key_last4: key.key_last4,
    is_enabled: key.is_enabled,
    created_at: key.created_at,
    updated_at: key.updated_at,
    checkStatus: "unchecked",
  };
}

export function createEmptyEditingProviderData(): EditingProviderData {
  return {
    id: null,
    name: "",
    provider_key: "",
    is_enabled: true,
    provider_api_key_mode: "QUEUE",
    upstream_sources: [],
    models: [],
    provider_keys: [],
    request_patches: [],
  };
}

export function buildProviderBootstrapPayload(
  form: ProviderBootstrapFormState,
  saveAndTest = false,
): ProviderBootstrapPayload {
  const payload: ProviderBootstrapPayload = {
    initial_source: {
      profile_type: trimText(form.profile_type),
      endpoint: trimText(form.endpoint),
      use_proxy: !!form.use_proxy,
      is_enabled: true,
      is_default: true,
    },
    api_key: trimText(form.api_key),
    model_name: trimText(form.model_name),
    key: trimText(form.key ?? form.provider_key),
    save_and_test: !!saveAndTest,
  };

  const providerName = trimText(form.name ?? form.provider_name);
  if (providerName) {
    payload.name = providerName;
  }

  const realModelName = trimText(form.real_model_name);
  if (realModelName) {
    payload.real_model_name = realModelName;
  }

  const apiKeyDescription = trimText(form.api_key_description);
  if (apiKeyDescription) {
    payload.api_key_description = apiKeyDescription;
  }

  return payload;
}

export function buildProviderUpdatePayload(
  editingData: EditingProviderData,
  form: ProviderBootstrapFormState,
): ProviderUpdatePayload {
  return {
    name: trimText(form.provider_name) || trimText(editingData.name),
    is_enabled: editingData.is_enabled,
    provider_api_key_mode: editingData.provider_api_key_mode,
  };
}

export function buildProviderBootstrapPreview(
  form: ProviderBootstrapPreviewState,
  response: ProviderBootstrapResponse | null = null,
): {
  provider_name: string;
  provider_key: string;
} {
  const providerName =
    trimText(response?.provider_name) ||
    trimText(response?.provider?.name) ||
    trimText(form.provider_name ?? form.name) ||
    titleize(form.profile_type) ||
    "Provider";

  const providerKey =
    trimText(response?.provider_key) ||
    trimText(response?.provider?.provider_key) ||
    trimText(form.provider_key ?? form.key);

  return {
    provider_name: providerName,
    provider_key: providerKey,
  };
}

export function hydrateEditingProviderDataFromBootstrap(
  editingData: EditingProviderData,
  response: ProviderBootstrapResponse | null | undefined,
): EditingProviderData {
  if (!editingData || !response) {
    return editingData;
  }

  const provider = response.provider;
  const source = getPreferredSource(editingData);
  const preview = buildProviderBootstrapPreview(
    {
      profile_type: source?.profile_type ?? "OPENAI",
      endpoint: source?.endpoint ?? "",
      provider_name: editingData.name,
      provider_key: editingData.provider_key,
    },
    response,
  );

  if (provider?.id !== undefined && provider.id !== null) {
    editingData.id = provider.id;
  }

  editingData.name =
    trimText(provider?.name) || preview.provider_name || editingData.name;
  editingData.provider_key =
    trimText(provider?.provider_key) ||
    preview.provider_key ||
    editingData.provider_key;
  if (provider) {
    editingData.is_enabled = provider.is_enabled;
    editingData.provider_api_key_mode = provider.provider_api_key_mode;
    editingData.upstream_sources = provider.upstream_sources.map((source) => ({
      id: source.id,
      provider_id: source.provider_id,
      profile_type: trimText(source.profile_type),
      endpoint: trimText(source.endpoint),
      use_proxy: source.use_proxy,
      is_enabled: source.is_enabled,
      is_default: source.is_default,
      deleted_at: source.deleted_at,
      created_at: source.created_at,
      updated_at: source.updated_at,
    }));
  }

  if (response.created_key) {
    const normalizedKey = mapProviderApiKeySummary(response.created_key);
    if (!normalizedKey) return editingData;
    const existingIndex = editingData.provider_keys.findIndex(
      (item) => item.id === normalizedKey.id,
    );

    if (existingIndex >= 0) {
      editingData.provider_keys.splice(existingIndex, 1, normalizedKey);
    } else {
      editingData.provider_keys.push(normalizedKey);
    }
  }

  if (response.created_model) {
    const normalizedModel = mapCreatedModel(response.created_model);
    const existingIndex =
      normalizedModel.id === null
        ? -1
        : editingData.models.findIndex((item) => item.id === normalizedModel.id);

    if (existingIndex >= 0) {
      editingData.models.splice(existingIndex, 1, normalizedModel);
    } else {
      editingData.models.push(normalizedModel);
    }
  }

  return editingData;
}

export function normalizeBootstrapCheckResult(checkResult: unknown): {
  ok: boolean;
  message: string;
} | null {
  if (checkResult === null || checkResult === undefined) {
    return null;
  }

  if (typeof checkResult === "boolean") {
    return {
      ok: checkResult,
      message: "",
    };
  }

  if (typeof checkResult === "string") {
    return {
      ok: false,
      message: checkResult,
    };
  }

  if (Array.isArray(checkResult)) {
    return {
      ok: checkResult.length === 0,
      message: checkResult.join(", "),
    };
  }

  if (typeof checkResult === "object") {
    const record = checkResult as Record<string, unknown>;
    if ("ok" in record) {
      return {
        ok: !!record.ok,
        message: trimText(record.message ?? record.error ?? ""),
      };
    }

    if ("success" in record) {
      return {
        ok: !!record.success,
        message: trimText(record.message ?? record.error ?? ""),
      };
    }

    if ("error" in record && record.error) {
      return {
        ok: false,
        message: String(record.error),
      };
    }

    if ("message" in record && record.message) {
      return {
        ok: true,
        message: String(record.message),
      };
    }
  }

  return {
    ok: true,
    message: "",
  };
}
