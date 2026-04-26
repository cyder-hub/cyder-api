import type { Ref } from "vue";

import type { ReasoningProfileCatalog, ReasoningProfileItem } from "./types";

export interface ReasoningProfileLoadState {
  profiles: Ref<ReasoningProfileItem[]>;
  catalog: Ref<ReasoningProfileCatalog | null>;
  loaded: Ref<boolean>;
  loading: Ref<boolean>;
  error: Ref<string | null>;
}

export interface ReasoningProfileApiClient {
  getReasoningProfileCatalog(): Promise<ReasoningProfileCatalog>;
  getReasoningProfileList(): Promise<ReasoningProfileItem[]>;
}

export type ReasoningProfileErrorNormalizer = (error: unknown) => Error;

export function shouldFetchReasoningProfiles(state: ReasoningProfileLoadState): boolean {
  return !state.loaded.value || !state.catalog.value;
}

export async function fetchReasoningProfiles(
  state: ReasoningProfileLoadState,
  client: ReasoningProfileApiClient,
  normalizeError: ReasoningProfileErrorNormalizer,
): Promise<ReasoningProfileItem[]> {
  state.loading.value = true;
  state.error.value = null;
  try {
    const [catalogData, profileData] = await Promise.all([
      client.getReasoningProfileCatalog(),
      client.getReasoningProfileList(),
    ]);
    state.catalog.value = catalogData;
    state.profiles.value = profileData ?? [];
    state.loaded.value = true;
    return state.profiles.value;
  } catch (err) {
    const normalizedError = normalizeError(err);
    state.error.value = normalizedError.message;
    throw normalizedError;
  } finally {
    state.loading.value = false;
  }
}

export async function ensureReasoningProfiles(
  state: ReasoningProfileLoadState,
  client: ReasoningProfileApiClient,
  normalizeError: ReasoningProfileErrorNormalizer,
) {
  if (shouldFetchReasoningProfiles(state)) {
    await fetchReasoningProfiles(state, client, normalizeError);
  }
  return {
    catalog: state.catalog.value,
    profiles: state.profiles.value,
  };
}
