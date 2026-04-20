import type {
  ProviderApiKeyItem,
  ProviderBootstrapPayload,
  ProviderBootstrapResponse,
  ModelItem,
  ProviderPayload,
} from "@/store/types";
import type {
  EditingProviderData,
  LocalEditableModelItem,
  LocalProviderApiKeyItem,
} from "@/components/provider/types";

export interface ProviderBootstrapPreviewState {
  provider_type: string;
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
  real_model_name?: string | null;
}

export function createProviderBootstrapFormState(
  editingData?: Partial<EditingProviderData> | null,
): ProviderBootstrapFormState {
  return {
    provider_type: trimText(editingData?.provider_type) || "OPENAI",
    endpoint: trimText(editingData?.endpoint),
    api_key: "",
    model_name: "",
    api_key_description: "",
    use_proxy: editingData?.use_proxy ?? false,
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

  form.provider_type = trimText(editingData.provider_type) || "OPENAI";
  form.endpoint = trimText(editingData.endpoint);
  form.use_proxy = editingData.use_proxy ?? false;
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

function slugify(value: unknown): string {
  const text = trimText(value);
  if (!text) return "";

  return text
    .toLowerCase()
    .replace(/['"]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function getEndpointHost(endpoint: unknown): string {
  const text = trimText(endpoint);
  if (!text) return "";

  try {
    return new URL(text).hostname;
  } catch {
    return "";
  }
}

function buildPreviewSubject(form: ProviderBootstrapPreviewState): string {
  return (
    trimText(form.model_name) ||
    getEndpointHost(form.endpoint) ||
    trimText(form.provider_key ?? form.key) ||
    "provider"
  );
}

function mapCreatedModel(
  model:
    | Partial<Pick<ModelItem, "id" | "model_name" | "real_model_name" | "is_enabled">>
    | null
    | undefined,
): LocalEditableModelItem {
  return {
    id: model?.id ?? null,
    model_name: model?.model_name ?? "",
    real_model_name: model?.real_model_name ?? null,
    is_enabled: model?.is_enabled ?? true,
    isEditing: false,
    checkStatus: "unchecked",
  };
}

function mapCreatedApiKey(
  key:
    | Partial<Pick<ProviderApiKeyItem, "id" | "api_key" | "description">>
    | null
    | undefined,
): LocalProviderApiKeyItem {
  return {
    id: key?.id ?? null,
    api_key: key?.api_key ?? "",
    description: key?.description ?? null,
    isEditing: false,
    checkStatus: "unchecked",
  };
}

export function createEmptyEditingProviderData(): EditingProviderData {
  return {
    id: null,
    name: "",
    provider_key: "",
    provider_type: "OPENAI",
    endpoint: "",
    use_proxy: false,
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
    endpoint: trimText(form.endpoint),
    api_key: trimText(form.api_key),
    model_name: trimText(form.model_name),
    save_and_test: !!saveAndTest,
    use_proxy: !!form.use_proxy,
  };

  const providerType = trimText(form.provider_type);
  if (providerType) {
    payload.provider_type = providerType;
  }

  const providerName = trimText(form.name ?? form.provider_name);
  if (providerName) {
    payload.name = providerName;
  }

  const providerKey = trimText(form.key ?? form.provider_key);
  if (providerKey) {
    payload.key = providerKey;
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
): ProviderPayload {
  return {
    key: trimText(editingData.provider_key),
    name: trimText(form.provider_name) || trimText(editingData.name),
    endpoint: trimText(form.endpoint),
    use_proxy: !!form.use_proxy,
    provider_type: trimText(form.provider_type) || trimText(editingData.provider_type),
    omit_config: null,
    api_keys: [],
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
    titleize(form.provider_type) ||
    "Provider";

  const providerKey =
    trimText(response?.provider_key) ||
    trimText(response?.provider?.provider_key) ||
    trimText(form.provider_key ?? form.key) ||
    (() => {
      const providerTypeSlug = slugify(form.provider_type) || "provider";
      const endpointHost = slugify(getEndpointHost(form.endpoint));
      const subject = slugify(buildPreviewSubject(form));
      const suffix = endpointHost || subject;
      return suffix ? `${providerTypeSlug}-${suffix}` : providerTypeSlug;
    })();

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
  const preview = buildProviderBootstrapPreview(editingData, response);

  if (provider?.id !== undefined && provider.id !== null) {
    editingData.id = provider.id;
  }

  editingData.name =
    trimText(provider?.name) || preview.provider_name || editingData.name;
  editingData.provider_key =
    trimText(provider?.provider_key) ||
    preview.provider_key ||
    editingData.provider_key;
  editingData.provider_type =
    trimText(provider?.provider_type) || editingData.provider_type;
  editingData.endpoint = trimText(provider?.endpoint) || editingData.endpoint;

  if (typeof provider?.use_proxy === "boolean") {
    editingData.use_proxy = provider.use_proxy;
  }

  if (response.created_key) {
    const normalizedKey = mapCreatedApiKey(response.created_key);
    const existingIndex =
      normalizedKey.id === null
        ? -1
        : editingData.provider_keys.findIndex(
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
