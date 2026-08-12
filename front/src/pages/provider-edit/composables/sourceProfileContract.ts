import type {
  UpstreamProfileType,
  UpstreamSource,
  UpstreamSourcePayload,
  UpstreamSourceUpdatePayload,
} from "@/services/types";

export const providerProfileTypes: readonly UpstreamProfileType[] = [
  "OPENAI",
  "OPENAI_COMPATIBLE",
  "GEMINI_OPENAI",
  "GEMINI",
  "VERTEX",
  "ANTHROPIC",
  "RESPONSES",
  "OLLAMA",
];

export type SourceOperation = "chat_completions" | "embeddings" | "rerank";

export interface ProviderSourceDraft {
  profile_type: UpstreamProfileType;
  base_url: string;
  use_proxy: boolean;
  chat_completions_enabled: boolean;
  chat_completions_path_override: string;
  embeddings_enabled: boolean;
  embeddings_path_override: string;
  rerank_enabled: boolean;
  rerank_path_override: string;
  is_enabled: boolean;
  is_default: boolean;
}

const officialDefaultProfiles = new Set<UpstreamProfileType>([
  "OPENAI",
  "GEMINI_OPENAI",
]);

export const sourceSupportsOperation = (
  profile: UpstreamProfileType,
  operation: SourceOperation,
) => {
  if (profile === "OPENAI_COMPATIBLE") return true;
  if (profile === "OPENAI" || profile === "GEMINI_OPENAI") {
    return operation !== "rerank";
  }
  return false;
};

export const sourceBaseUrlMayBeEmpty = (profile: UpstreamProfileType) =>
  officialDefaultProfiles.has(profile);

export const createProviderSourceDraft = (
  source?: UpstreamSource | null,
): ProviderSourceDraft => {
  const profile = source?.profile_type ?? "OPENAI";
  return {
    profile_type: profile,
    base_url: source?.base_url ?? "",
    use_proxy: source?.use_proxy ?? false,
    chat_completions_enabled:
      source?.chat_completions_enabled ?? sourceSupportsOperation(profile, "chat_completions"),
    chat_completions_path_override: source?.chat_completions_path_override ?? "",
    embeddings_enabled:
      source?.embeddings_enabled ??
      (profile === "OPENAI" || profile === "GEMINI_OPENAI"),
    embeddings_path_override: source?.embeddings_path_override ?? "",
    rerank_enabled: source?.rerank_enabled ?? false,
    rerank_path_override: source?.rerank_path_override ?? "",
    is_enabled: source?.is_enabled ?? true,
    is_default: source?.is_default ?? false,
  };
};

export const applySourceProfileDefaults = (
  draft: ProviderSourceDraft,
  profile: UpstreamProfileType,
) => {
  const defaults = createProviderSourceDraft({ profile_type: profile } as UpstreamSource);
  draft.profile_type = profile;
  draft.chat_completions_enabled = defaults.chat_completions_enabled;
  draft.chat_completions_path_override = "";
  draft.embeddings_enabled = defaults.embeddings_enabled;
  draft.embeddings_path_override = "";
  draft.rerank_enabled = false;
  draft.rerank_path_override = "";
};

const optionalPath = (path: string) => path.trim() || undefined;
const updatePath = (path: string) => path.trim() || null;

export const buildSourceCreatePayload = (
  draft: ProviderSourceDraft,
): UpstreamSourcePayload => {
  const profileType = draft.profile_type.trim() as UpstreamProfileType;
  const lifecycle = {
    use_proxy: draft.use_proxy,
    is_enabled: draft.is_enabled,
    is_default: draft.is_default,
  };
  const baseUrl = draft.base_url.trim();

  if (profileType === "OPENAI" || profileType === "GEMINI_OPENAI") {
    return {
      ...lifecycle,
      profile_type: profileType,
      ...(baseUrl ? { base_url: baseUrl } : {}),
      chat_completions_enabled: draft.chat_completions_enabled,
      ...(optionalPath(draft.chat_completions_path_override)
        ? { chat_completions_path_override: optionalPath(draft.chat_completions_path_override) }
        : {}),
      embeddings_enabled: draft.embeddings_enabled,
      ...(optionalPath(draft.embeddings_path_override)
        ? { embeddings_path_override: optionalPath(draft.embeddings_path_override) }
        : {}),
    };
  }
  if (profileType === "OPENAI_COMPATIBLE") {
    return {
      ...lifecycle,
      profile_type: profileType,
      base_url: baseUrl,
      chat_completions_enabled: draft.chat_completions_enabled,
      ...(optionalPath(draft.chat_completions_path_override)
        ? { chat_completions_path_override: optionalPath(draft.chat_completions_path_override) }
        : {}),
      embeddings_enabled: draft.embeddings_enabled,
      ...(optionalPath(draft.embeddings_path_override)
        ? { embeddings_path_override: optionalPath(draft.embeddings_path_override) }
        : {}),
      rerank_enabled: draft.rerank_enabled,
      ...(optionalPath(draft.rerank_path_override)
        ? { rerank_path_override: optionalPath(draft.rerank_path_override) }
        : {}),
    };
  }
  return {
    ...lifecycle,
    profile_type: profileType,
    base_url: baseUrl,
  } as UpstreamSourcePayload;
};

export const buildSourceUpdatePayload = (
  draft: ProviderSourceDraft,
): UpstreamSourceUpdatePayload => {
  const payload: UpstreamSourceUpdatePayload = {
    base_url: draft.base_url.trim() || null,
    use_proxy: draft.use_proxy,
    is_enabled: draft.is_enabled,
    is_default: draft.is_default,
  };
  if (sourceSupportsOperation(draft.profile_type, "chat_completions")) {
    payload.chat_completions_enabled = draft.chat_completions_enabled;
    payload.chat_completions_path_override = updatePath(
      draft.chat_completions_path_override,
    );
  }
  if (sourceSupportsOperation(draft.profile_type, "embeddings")) {
    payload.embeddings_enabled = draft.embeddings_enabled;
    payload.embeddings_path_override = updatePath(draft.embeddings_path_override);
  }
  if (sourceSupportsOperation(draft.profile_type, "rerank")) {
    payload.rerank_enabled = draft.rerank_enabled;
    payload.rerank_path_override = updatePath(draft.rerank_path_override);
  }
  return payload;
};
