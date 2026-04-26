import { computed, ref } from "vue";
import { defineStore } from "pinia";

import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type { ReasoningProfileCatalog, ReasoningProfileItem } from "./types";
import {
  ensureReasoningProfiles,
  fetchReasoningProfiles,
  type ReasoningProfileLoadState,
} from "./reasoningProfileLoadState";

export const useReasoningProfileStore = defineStore("reasoningProfile", () => {
  const profiles = ref<ReasoningProfileItem[]>([]);
  const catalog = ref<ReasoningProfileCatalog | null>(null);
  const loaded = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const loadState: ReasoningProfileLoadState = {
    profiles,
    catalog,
    loaded,
    loading,
    error,
  };

  async function fetchAll() {
    return fetchReasoningProfiles(loadState, Api, normalizeError);
  }

  async function ensureLoaded() {
    return ensureReasoningProfiles(loadState, Api, normalizeError);
  }

  const enabledProfiles = computed(() =>
    profiles.value.filter((item) => item.profile.is_enabled),
  );

  const profileOptions = computed(() =>
    enabledProfiles.value.map((item) => ({
      value: item.profile.id,
      label: `${item.profile.name} (${item.profile.profile_key})`,
      family: item.family,
    })),
  );

  const profileById = computed(() => {
    const map = new Map<number, ReasoningProfileItem>();
    for (const item of profiles.value) {
      map.set(item.profile.id, item);
    }
    return map;
  });

  const getProfileById = (id: number | string | null | undefined) => {
    if (id === null || id === undefined || id === "") return null;
    const numericId = Number(id);
    return Number.isFinite(numericId) ? profileById.value.get(numericId) ?? null : null;
  };

  return {
    profiles,
    catalog,
    loaded,
    loading,
    error,
    enabledProfiles,
    profileOptions,
    profileById,
    getProfileById,
    fetchAll,
    ensureLoaded,
  };
});
