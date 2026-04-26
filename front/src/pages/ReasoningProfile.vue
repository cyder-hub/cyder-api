<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import {
  BrainCircuit,
  Loader2,
  Pencil,
  Plus,
  RefreshCcw,
  Settings2,
  Trash2,
} from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { confirm } from "@/lib/confirmController";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { Api } from "@/services/request";
import { useReasoningProfileStore } from "@/store/reasoningProfileStore";
import type {
  ReasoningPresetKey,
  ReasoningProfileItem,
  ReasoningProfilePayload,
  ReasoningProfilePresetRow,
} from "@/store/types";

type ProfileForm = {
  id: number | null;
  profile_key: string;
  name: string;
  description: string;
  family_key: string;
  is_enabled: boolean;
};

const store = useReasoningProfileStore();
const isProfileDialogOpen = ref(false);
const isPresetDialogOpen = ref(false);
const savingProfile = ref(false);
const presetBusyKey = ref<string | null>(null);
const selectedProfile = ref<ReasoningProfileItem | null>(null);

const form = reactive<ProfileForm>({
  id: null,
  profile_key: "",
  name: "",
  description: "",
  family_key: "openai_chat_reasoning_effort",
  is_enabled: true,
});

const summaryCards = computed(() => {
  const total = store.profiles.length;
  const enabled = store.profiles.filter((item) => item.profile.is_enabled).length;
  const presets = store.profiles.reduce((sum, item) => sum + item.presets.length, 0);
  const bindings = store.profiles.reduce(
    (sum, item) => sum + item.provider_bindings.length + item.model_bindings.length,
    0,
  );
  return [
    { key: "total", label: "Profiles", value: total },
    { key: "enabled", label: "Enabled", value: enabled },
    { key: "presets", label: "Preset rows", value: presets },
    { key: "bindings", label: "Bindings", value: bindings },
  ];
});

const familyOptions = computed(() => store.catalog?.families ?? []);
const presetMetadata = computed(() => store.catalog?.presets ?? []);

const resetForm = () => {
  form.id = null;
  form.profile_key = "";
  form.name = "";
  form.description = "";
  form.family_key = familyOptions.value[0]?.family_key || "openai_chat_reasoning_effort";
  form.is_enabled = true;
};

const loadData = async () => {
  try {
    await store.fetchAll();
  } catch (error: unknown) {
    toastController.error(normalizeError(error, "Failed to load reasoning profiles").message);
  }
};

const openCreateDialog = () => {
  resetForm();
  isProfileDialogOpen.value = true;
};

const openEditDialog = (profile: ReasoningProfileItem) => {
  form.id = profile.profile.id;
  form.profile_key = profile.profile.profile_key;
  form.name = profile.profile.name;
  form.description = profile.profile.description || "";
  form.family_key = profile.family;
  form.is_enabled = profile.profile.is_enabled;
  isProfileDialogOpen.value = true;
};

const saveProfile = async () => {
  if (!form.profile_key.trim() || !form.name.trim()) {
    toastController.warn("Profile key and name are required.");
    return;
  }

  const payload: ReasoningProfilePayload = {
    profile_key: form.profile_key.trim(),
    name: form.name.trim(),
    description: form.description.trim() || null,
    family_key: form.family_key,
    is_enabled: form.is_enabled,
  };

  savingProfile.value = true;
  try {
    if (form.id) {
      await Api.updateReasoningProfile(form.id, payload);
    } else {
      await Api.createReasoningProfile(payload);
    }
    isProfileDialogOpen.value = false;
    await loadData();
  } catch (error: unknown) {
    toastController.error(normalizeError(error, "Failed to save reasoning profile").message);
  } finally {
    savingProfile.value = false;
  }
};

const deleteProfile = async (profile: ReasoningProfileItem) => {
  if (
    !(await confirm({
      title: `Delete reasoning profile ${profile.profile.profile_key}?`,
    }))
  ) {
    return;
  }
  try {
    await Api.deleteReasoningProfile(profile.profile.id);
    await loadData();
  } catch (error: unknown) {
    toastController.error(normalizeError(error, "Failed to delete reasoning profile").message);
  }
};

const openPresetDialog = (profile: ReasoningProfileItem) => {
  selectedProfile.value = profile;
  isPresetDialogOpen.value = true;
};

const presetRowByKey = (profile: ReasoningProfileItem, presetKey: ReasoningPresetKey) =>
  profile.presets.find((item) => item.preset_key === presetKey) || null;

const familySupportsPreset = (profile: ReasoningProfileItem, presetKey: ReasoningPresetKey) => {
  const family = familyOptions.value.find((item) => item.family_key === profile.family);
  return family?.supported_presets.includes(presetKey) ?? false;
};

const togglePreset = async (
  profile: ReasoningProfileItem,
  presetKey: ReasoningPresetKey,
  patch: Partial<Pick<ReasoningProfilePresetRow, "is_enabled" | "expose_in_models">>,
) => {
  const existing = presetRowByKey(profile, presetKey);
  const next = {
    preset_key: presetKey,
    is_enabled: patch.is_enabled ?? existing?.is_enabled ?? true,
    expose_in_models: patch.expose_in_models ?? existing?.expose_in_models ?? true,
  };

  presetBusyKey.value = `${profile.profile.id}:${presetKey}`;
  try {
    await Api.upsertReasoningProfilePreset(profile.profile.id, next);
    await loadData();
    selectedProfile.value =
      store.profiles.find((item) => item.profile.id === profile.profile.id) || null;
  } catch (error: unknown) {
    toastController.error(normalizeError(error, "Failed to update preset").message);
  } finally {
    presetBusyKey.value = null;
  }
};

onMounted(() => {
  void loadData();
});
</script>

<template>
  <CrudPageLayout
    title="Reasoning profiles"
    description="Configure suffix presets, provider defaults, and model overrides for gateway routing."
    :loading="store.loading"
    :error="store.error"
    :empty="!store.profiles.length"
    content-class="space-y-4"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" @click="loadData">
        <RefreshCcw class="mr-1.5 h-4 w-4" />
        Refresh
      </Button>
      <Button variant="default" class="w-full sm:w-auto" @click="openCreateDialog">
        <Plus class="mr-1.5 h-4 w-4" />
        Add profile
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center py-16 text-gray-400">
        <Loader2 class="mr-2 h-5 w-5 animate-spin" />
        <span class="text-sm">Loading reasoning profiles</span>
      </div>
    </template>

    <template #error="{ error }">
      <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ error }}
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20 text-gray-500">
        <BrainCircuit class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
        <p class="text-sm font-medium">No reasoning profiles configured.</p>
      </div>
    </template>

    <div class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-gray-200 bg-gray-100 sm:grid-cols-4">
      <div v-for="card in summaryCards" :key="card.key" class="bg-white px-4 py-3">
        <p class="text-[11px] font-medium uppercase tracking-wider text-gray-500">
          {{ card.label }}
        </p>
        <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
          {{ card.value }}
        </p>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-3 md:hidden">
      <MobileCrudCard
        v-for="profile in store.profiles"
        :key="profile.profile.id"
        :title="profile.profile.name"
        :description="profile.profile.profile_key"
      >
        <template #header>
          <Badge :variant="profile.profile.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
            {{ profile.profile.is_enabled ? "enabled" : "disabled" }}
          </Badge>
        </template>

        <div class="space-y-2 text-xs text-gray-500">
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>Family</span>
            <span class="font-mono text-gray-700">{{ profile.family }}</span>
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>Presets</span>
            <span class="font-medium text-gray-700">{{ profile.presets.length }}</span>
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>Bindings</span>
            <span class="font-medium text-gray-700">
              {{ profile.provider_bindings.length + profile.model_bindings.length }}
            </span>
          </div>
        </div>

        <template #actions>
          <Button variant="ghost" size="sm" class="w-full justify-center" @click="openPresetDialog(profile)">
            <Settings2 class="mr-1.5 h-3.5 w-3.5" />
            Presets
          </Button>
          <Button variant="ghost" size="sm" class="w-full justify-center" @click="openEditDialog(profile)">
            <Pencil class="mr-1.5 h-3.5 w-3.5" />
            Edit
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-400 hover:text-red-600"
            @click="deleteProfile(profile)"
          >
            <Trash2 class="mr-1.5 h-3.5 w-3.5" />
            Delete
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden overflow-hidden rounded-lg border border-gray-200 bg-white md:block">
      <Table>
        <TableHeader>
          <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Profile</TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Family</TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Presets</TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Bindings</TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Enabled</TableHead>
            <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="profile in store.profiles" :key="profile.profile.id">
            <TableCell>
              <div class="min-w-0">
                <div class="font-medium text-gray-900">{{ profile.profile.name }}</div>
                <div class="mt-0.5 break-all font-mono text-xs text-gray-500">
                  {{ profile.profile.profile_key }}
                </div>
              </div>
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">{{ profile.family }}</TableCell>
            <TableCell>
              <div class="flex max-w-md flex-wrap gap-1.5">
                <Badge
                  v-for="preset in profile.presets"
                  :key="preset.id"
                  :variant="preset.is_enabled ? 'secondary' : 'outline'"
                  class="font-mono text-[11px]"
                >
                  {{ preset.preset_key }} -{{ preset.suffix }}
                </Badge>
              </div>
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">
              P{{ profile.provider_bindings.length }} / M{{ profile.model_bindings.length }}
            </TableCell>
            <TableCell>
              <Badge :variant="profile.profile.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
                {{ profile.profile.is_enabled ? "yes" : "no" }}
              </Badge>
            </TableCell>
            <TableCell class="text-right">
              <Button variant="ghost" size="sm" @click="openPresetDialog(profile)">
                <Settings2 class="mr-1 h-3.5 w-3.5" />
                Presets
              </Button>
              <Button variant="ghost" size="sm" @click="openEditDialog(profile)">
                <Pencil class="mr-1 h-3.5 w-3.5" />
                Edit
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600"
                @click="deleteProfile(profile)"
              >
                <Trash2 class="mr-1 h-3.5 w-3.5" />
                Delete
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <template #modals>
      <Dialog :open="isProfileDialogOpen" @update:open="(open) => (isProfileDialogOpen = open)">
        <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-2xl">
          <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
            <DialogTitle>{{ form.id ? "Edit reasoning profile" : "Add reasoning profile" }}</DialogTitle>
          </DialogHeader>
          <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
            <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div class="space-y-1.5">
                <Label>Profile key <span class="ml-0.5 text-red-500">*</span></Label>
                <Input v-model="form.profile_key" class="font-mono text-sm" />
              </div>
              <div class="space-y-1.5">
                <Label>Name <span class="ml-0.5 text-red-500">*</span></Label>
                <Input v-model="form.name" />
              </div>
            </div>

            <div class="space-y-1.5">
              <Label>Family</Label>
              <Select v-model="form.family_key">
                <SelectTrigger class="w-full">
                  <SelectValue placeholder="Select family" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="family in familyOptions"
                    :key="family.family_key"
                    :value="family.family_key"
                  >
                    {{ family.family_key }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div class="space-y-1.5">
              <Label>Description</Label>
              <textarea
                v-model="form.description"
                class="min-h-24 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200"
              />
            </div>

            <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
              <Label class="cursor-pointer">Enabled</Label>
              <Checkbox v-model="form.is_enabled" />
            </div>
          </div>
          <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
            <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" @click="isProfileDialogOpen = false">
              Cancel
            </Button>
            <Button variant="default" class="w-full sm:w-auto" :disabled="savingProfile" @click="saveProfile">
              <Loader2 v-if="savingProfile" class="mr-1.5 h-4 w-4 animate-spin" />
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog :open="isPresetDialogOpen" @update:open="(open) => (isPresetDialogOpen = open)">
        <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-5xl">
          <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
            <DialogTitle>
              Presets
              <span v-if="selectedProfile" class="font-mono text-sm font-normal text-gray-500">
                {{ selectedProfile.profile.profile_key }}
              </span>
            </DialogTitle>
          </DialogHeader>

          <div v-if="selectedProfile" class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
            <div class="overflow-hidden rounded-lg border border-gray-200">
              <Table>
                <TableHeader>
                  <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                    <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Preset</TableHead>
                    <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Suffix</TableHead>
                    <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Requires reasoning</TableHead>
                    <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Operations</TableHead>
                    <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">Enabled</TableHead>
                    <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">/models</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <TableRow v-for="preset in presetMetadata" :key="preset.preset_key">
                    <TableCell>
                      <div class="flex items-center gap-2">
                        <span class="font-mono text-sm text-gray-900">{{ preset.preset_key }}</span>
                        <Badge
                          v-if="!familySupportsPreset(selectedProfile, preset.preset_key)"
                          variant="outline"
                          class="font-mono text-[10px] text-gray-500"
                        >
                          unsupported
                        </Badge>
                      </div>
                    </TableCell>
                    <TableCell class="font-mono text-sm text-gray-700">-{{ preset.suffix }}</TableCell>
                    <TableCell>
                      <Badge :variant="preset.requires_reasoning ? 'secondary' : 'outline'" class="font-mono text-[11px]">
                        {{ preset.requires_reasoning ? "yes" : "no" }}
                      </Badge>
                    </TableCell>
                    <TableCell class="font-mono text-xs text-gray-600">
                      {{ preset.allowed_operation_kinds.join(", ") }}
                    </TableCell>
                    <TableCell>
                      <Checkbox
                        :model-value="presetRowByKey(selectedProfile, preset.preset_key)?.is_enabled ?? false"
                        :disabled="
                          !familySupportsPreset(selectedProfile, preset.preset_key) ||
                          presetBusyKey === `${selectedProfile.profile.id}:${preset.preset_key}`
                        "
                        @update:model-value="
                          (checked) =>
                            togglePreset(selectedProfile!, preset.preset_key, {
                              is_enabled: checked === true,
                            })
                        "
                      />
                    </TableCell>
                    <TableCell>
                      <Checkbox
                        :model-value="presetRowByKey(selectedProfile, preset.preset_key)?.expose_in_models ?? false"
                        :disabled="
                          !familySupportsPreset(selectedProfile, preset.preset_key) ||
                          !(presetRowByKey(selectedProfile, preset.preset_key)?.is_enabled ?? false) ||
                          presetBusyKey === `${selectedProfile.profile.id}:${preset.preset_key}`
                        "
                        @update:model-value="
                          (checked) =>
                            togglePreset(selectedProfile!, preset.preset_key, {
                              expose_in_models: checked === true,
                            })
                        "
                      />
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </div>
          </div>

          <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
            <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" @click="isPresetDialogOpen = false">
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </template>
  </CrudPageLayout>
</template>
