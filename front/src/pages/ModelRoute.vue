<template>
  <CrudPageLayout
    :title="$t('modelRoutePage.title')"
    :description="$t('modelRoutePage.description')"
    :loading="loading"
    :error="error"
    :empty="!routes.length"
  >
    <template #actions>
      <Button @click="handleOpenAddModal" variant="outline" :disabled="loading">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("modelRoutePage.addRoute") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("modelRoutePage.loading") }}</span>
      </div>
    </template>

    <template #error="{ error }">
      <div class="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-white py-20">
        <AlertCircle class="mb-4 h-10 w-10 stroke-1 text-red-400" />
        <span class="text-sm font-medium text-red-500">
          {{ $t("modelRoutePage.errorPrefix") }} {{ error }}
        </span>
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-white py-20">
        <Inbox class="mb-4 h-10 w-10 stroke-1 text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ $t("modelRoutePage.noData") }}
        </span>
      </div>
    </template>

    <div class="grid grid-cols-1 gap-3 md:hidden">
      <MobileCrudCard
        v-for="item in routes"
        :key="item.route.id"
        :title="item.route.route_name"
        :description="item.route.description || `${item.candidate_count} candidates`"
      >
        <template #header>
          <div class="flex items-center gap-2">
            <Badge :variant="item.route.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
              {{ item.route.is_enabled ? $t("common.yes") : $t("common.no") }}
            </Badge>
            <Badge :variant="item.route.expose_in_models ? 'secondary' : 'outline'" class="font-mono text-[11px]">
              /models: {{ item.route.expose_in_models ? $t("common.yes") : $t("common.no") }}
            </Badge>
          </div>
        </template>

        <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("modelRoutePage.table.candidateCount") }}</span>
            <span class="font-medium text-gray-700">{{ item.candidate_count }}</span>
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("modelRoutePage.table.enabled") }}</span>
            <Checkbox
              :model-value="item.route.is_enabled"
              @update:model-value="(value) => handleToggleEnabled(item, value === true)"
            />
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("modelRoutePage.table.exposeInModels") }}</span>
            <Checkbox
              :model-value="item.route.expose_in_models"
              @update:model-value="(value) => handleToggleExpose(item, value === true)"
            />
          </div>
        </div>

        <template #actions>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center"
            @click="handleOpenEditModal(item.route.id)"
          >
            <Pencil class="mr-1 h-3.5 w-3.5" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-400 hover:text-red-600"
            @click="handleDeleteRoute(item.route.id, item.route.route_name)"
          >
            <Trash2 class="mr-1 h-3.5 w-3.5" />
            {{ $t("common.delete") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden overflow-hidden rounded-lg border border-gray-200 bg-white md:block">
      <Table>
        <TableHeader>
          <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("modelRoutePage.table.routeName") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("modelRoutePage.table.candidateCount") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("modelRoutePage.table.exposeInModels") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("modelRoutePage.table.enabled") }}
            </TableHead>
            <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("common.actions") }}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="item in routes" :key="item.route.id">
            <TableCell>
              <div class="min-w-0">
                <div class="font-medium text-gray-900">{{ item.route.route_name }}</div>
                <div v-if="item.route.description" class="mt-0.5 text-xs text-gray-500">
                  {{ item.route.description }}
                </div>
              </div>
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">{{ item.candidate_count }}</TableCell>
            <TableCell>
              <Checkbox
                :model-value="item.route.expose_in_models"
                @update:model-value="(value) => handleToggleExpose(item, value === true)"
              />
            </TableCell>
            <TableCell>
              <Checkbox
                :model-value="item.route.is_enabled"
                @update:model-value="(value) => handleToggleEnabled(item, value === true)"
              />
            </TableCell>
            <TableCell class="text-right">
              <Button variant="ghost" size="sm" @click="handleOpenEditModal(item.route.id)">
                <Pencil class="mr-1 h-3.5 w-3.5" />
                {{ $t("common.edit") }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600"
                @click="handleDeleteRoute(item.route.id, item.route.route_name)"
              >
                <Trash2 class="mr-1 h-3.5 w-3.5" />
                {{ $t("common.delete") }}
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <template #modals>
      <Dialog :open="showEditModal" @update:open="setShowEditModal">
        <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-4xl">
          <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
            <DialogTitle class="text-lg font-semibold text-gray-900">
              {{ editingRoute.id ? $t("modelRoutePage.modal.titleEdit") : $t("modelRoutePage.modal.titleAdd") }}
            </DialogTitle>
          </DialogHeader>

          <div class="flex-1 space-y-5 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
            <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
              <div class="space-y-1.5">
                <Label class="text-gray-700">
                  {{ $t("modelRoutePage.modal.labelRouteName") }}
                  <span class="ml-0.5 text-red-500">*</span>
                </Label>
                <Input
                  v-model="editingRoute.route_name"
                  :placeholder="$t('modelRoutePage.modal.placeholderRouteName')"
                  class="font-mono text-sm"
                />
              </div>

              <div class="space-y-1.5">
                <Label class="text-gray-700">{{ $t("modelRoutePage.modal.labelDescription") }}</Label>
                <Input
                  v-model="editingRoute.description"
                  :placeholder="$t('modelRoutePage.modal.placeholderDescription')"
                />
              </div>
            </div>

            <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
                <Label class="cursor-pointer text-gray-700">
                  {{ $t("modelRoutePage.modal.labelExposeInModels") }}
                </Label>
                <Checkbox
                  v-model="editingRoute.expose_in_models"
                />
              </div>

              <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
                <Label class="cursor-pointer text-gray-700">
                  {{ $t("modelRoutePage.modal.labelEnabled") }}
                </Label>
                <Checkbox
                  v-model="editingRoute.is_enabled"
                />
              </div>
            </div>

            <section class="space-y-3">
              <div class="flex flex-col gap-3 border-b border-gray-100 pb-3 sm:flex-row sm:items-center sm:justify-between">
                <div>
                  <h3 class="text-base font-semibold text-gray-900">
                    {{ $t("modelRoutePage.modal.candidatesTitle") }}
                  </h3>
                  <p class="mt-1 text-sm text-gray-500">
                    {{ $t("modelRoutePage.description") }}
                  </p>
                </div>
                <Button variant="outline" class="w-full sm:w-auto" @click="handleAddCandidate">
                  <Plus class="mr-1.5 h-4 w-4" />
                  {{ $t("modelRoutePage.modal.addCandidate") }}
                </Button>
              </div>

              <div v-if="editingRoute.candidates.length === 0" class="rounded-lg border border-dashed border-gray-300 px-4 py-8 text-center text-sm text-gray-500">
                {{ $t("modelRoutePage.modal.emptyCandidates") }}
              </div>

              <div v-else class="space-y-3">
                <div
                  v-for="(candidate, index) in editingRoute.candidates"
                  :key="candidate.local_id"
                  class="rounded-lg border border-gray-200 bg-gray-50/40 p-3 sm:p-4"
                >
                  <div class="flex flex-col gap-3">
                    <div class="flex items-center justify-between gap-3">
                      <div class="flex items-center gap-2">
                        <Badge variant="outline" class="font-mono text-xs">
                          {{ $t("modelRoutePage.modal.candidateOrder") }} #{{ index + 1 }}
                        </Badge>
                        <Badge :variant="candidate.is_enabled ? 'secondary' : 'outline'" class="font-mono text-xs">
                          {{ candidate.is_enabled ? $t("common.yes") : $t("common.no") }}
                        </Badge>
                      </div>
                      <div class="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          :disabled="index === 0"
                          @click="moveCandidate(index, -1)"
                        >
                          <ArrowUp class="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          :disabled="index === editingRoute.candidates.length - 1"
                          @click="moveCandidate(index, 1)"
                        >
                          <ArrowDown class="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          class="text-gray-400 hover:text-red-600"
                          @click="removeCandidate(index)"
                        >
                          <Trash2 class="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    </div>

                    <div class="grid grid-cols-1 gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
                      <div class="space-y-1.5">
                        <Label class="text-gray-700">{{ $t("modelRoutePage.modal.candidateProvider") }}</Label>
                        <Select
                          :model-value="candidate.provider_id"
                          @update:model-value="(value) => handleCandidateProviderChange(index, value)"
                        >
                          <SelectTrigger class="w-full">
                            <SelectValue :placeholder="$t('modelRoutePage.modal.placeholderProvider')" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem
                              v-for="provider in providerOptions"
                              :key="provider.value"
                              :value="provider.value"
                            >
                              {{ provider.label }}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </div>

                      <div class="space-y-1.5">
                        <Label class="text-gray-700">{{ $t("modelRoutePage.modal.candidateModel") }}</Label>
                        <Select
                          :model-value="candidate.model_id"
                          :disabled="!candidate.provider_id"
                          @update:model-value="(value) => (candidate.model_id = asStringOrNull(value))"
                        >
                          <SelectTrigger class="w-full">
                            <SelectValue :placeholder="$t('modelRoutePage.modal.placeholderModel')" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem
                              v-for="model in getModelOptions(candidate.provider_id)"
                              :key="model.value"
                              :value="model.value"
                            >
                              {{ model.label }}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </div>

                      <div class="flex items-end">
                        <div class="flex w-full items-center justify-between rounded-lg border border-gray-200 bg-white px-3 py-2.5 lg:min-w-[140px]">
                          <Label class="cursor-pointer text-gray-700">
                            {{ $t("modelRoutePage.modal.candidateEnabled") }}
                          </Label>
                          <Checkbox v-model="candidate.is_enabled" />
                        </div>
                      </div>
                    </div>

                    <div class="rounded-lg border border-gray-200 bg-white px-3 py-2.5 text-xs text-gray-500">
                      {{ getCandidateSummary(candidate) }}
                    </div>
                  </div>
                </div>
              </div>
            </section>
          </div>

          <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
            <Button @click="handleCloseModal" variant="ghost" class="w-full text-gray-600 sm:w-auto">
              {{ $t("common.cancel") }}
            </Button>
            <Button @click="handleSaveRoute" variant="default" class="w-full sm:w-auto">
              {{ $t("common.save") }}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </template>
  </CrudPageLayout>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import { useProviderStore } from "@/store/providerStore";
import type {
  ModelRouteDetail,
  ModelRouteListItem,
  ModelRoutePayload,
  ModelRouteUpdatePayload,
  ProviderListItem,
} from "@/store/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { confirm } from "@/lib/confirmController";
import { toastController } from "@/lib/toastController";
import {
  AlertCircle,
  ArrowDown,
  ArrowUp,
  Inbox,
  Loader2,
  Pencil,
  Plus,
  Trash2,
} from "lucide-vue-next";

type EditingCandidate = {
  local_id: string;
  provider_id: string | null;
  model_id: string | null;
  is_enabled: boolean;
};

type EditingRoute = {
  id: number | null;
  route_name: string;
  description: string;
  is_enabled: boolean;
  expose_in_models: boolean;
  candidates: EditingCandidate[];
};

const { t: $t } = useI18n();
const providerStore = useProviderStore();

const routes = ref<ModelRouteListItem[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const showEditModal = ref(false);

const createCandidate = (): EditingCandidate => ({
  local_id: `${Date.now()}-${Math.random()}`,
  provider_id: null,
  model_id: null,
  is_enabled: true,
});

const createRouteTemplate = (): EditingRoute => ({
  id: null,
  route_name: "",
  description: "",
  is_enabled: true,
  expose_in_models: true,
  candidates: [createCandidate()],
});

const editingRoute = ref<EditingRoute>(createRouteTemplate());

const setShowEditModal = (value: boolean) => {
  showEditModal.value = value;
};

const asStringOrNull = (value: unknown): string | null =>
  typeof value === "string" && value.length > 0 ? value : null;

const providerOptions = computed(() =>
  providerStore.providers.map((provider) => ({
    value: String(provider.provider.id),
    label: provider.provider.name,
  })),
);

const getProviderById = (providerId: string | null): ProviderListItem | undefined => {
  if (!providerId) return undefined;
  return providerStore.providers.find(
    (provider) => String(provider.provider.id) === providerId,
  );
};

const getModelOptions = (providerId: string | null) => {
  const provider = getProviderById(providerId);
  if (!provider) return [];

  return (provider.models || []).map((item) => ({
    value: String(item.model.id),
    label: item.model.model_name,
  }));
};

const getCandidateSummary = (candidate: EditingCandidate) => {
  const provider = getProviderById(candidate.provider_id);
  const model = provider?.models.find(
    (item) => String(item.model.id) === candidate.model_id,
  );

  if (!provider || !model) {
    return $t("modelRoutePage.modal.emptyCandidates");
  }

  return `${provider.provider.provider_key}/${model.model.model_name}`;
};

const fetchRouteList = async () => {
  loading.value = true;
  error.value = null;
  try {
    routes.value = (await Api.getModelRouteList()) || [];
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    loading.value = false;
  }
};

onMounted(async () => {
  try {
    await providerStore.fetchProviders();
    await fetchRouteList();
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
    loading.value = false;
  }
});

const handleOpenAddModal = () => {
  editingRoute.value = createRouteTemplate();
  showEditModal.value = true;
};

const mapRouteDetailToEditingRoute = (detail: ModelRouteDetail): EditingRoute => ({
  id: detail.route.id,
  route_name: detail.route.route_name,
  description: detail.route.description || "",
  is_enabled: detail.route.is_enabled,
  expose_in_models: detail.route.expose_in_models,
  candidates: detail.candidates.map((candidate) => ({
    local_id: `${candidate.candidate.id}`,
    provider_id: String(candidate.provider_id),
    model_id: String(candidate.candidate.model_id),
    is_enabled: candidate.candidate.is_enabled,
  })),
});

const handleOpenEditModal = async (id: number) => {
  try {
    const detail = await Api.getModelRouteDetail(id);
    editingRoute.value = mapRouteDetailToEditingRoute(detail);
    showEditModal.value = true;
  } catch (err: unknown) {
    console.error("Failed to fetch model route detail:", err);
    toastController.error($t("modelRoutePage.alert.loadDetailFailed"));
  }
};

const handleCloseModal = () => {
  showEditModal.value = false;
};

const handleAddCandidate = () => {
  editingRoute.value.candidates.push(createCandidate());
};

const removeCandidate = (index: number) => {
  editingRoute.value.candidates.splice(index, 1);
};

const moveCandidate = (index: number, delta: -1 | 1) => {
  const targetIndex = index + delta;
  if (targetIndex < 0 || targetIndex >= editingRoute.value.candidates.length) {
    return;
  }

  const next = [...editingRoute.value.candidates];
  const [candidate] = next.splice(index, 1);
  next.splice(targetIndex, 0, candidate);
  editingRoute.value.candidates = next;
};

const handleCandidateProviderChange = (index: number, value: unknown) => {
  const providerId = asStringOrNull(value);
  const candidate = editingRoute.value.candidates[index];
  candidate.provider_id = providerId;
  candidate.model_id = null;
};

const buildCandidatePayload = () =>
  editingRoute.value.candidates.map((candidate, index) => ({
    model_id: Number(candidate.model_id),
    priority: index * 10,
    is_enabled: candidate.is_enabled,
  }));

const validateEditingRoute = () => {
  if (!editingRoute.value.route_name.trim()) {
    toastController.error($t("modelRoutePage.alert.routeNameRequired"));
    return false;
  }

  if (editingRoute.value.candidates.length === 0) {
    toastController.error($t("modelRoutePage.alert.candidateRequired"));
    return false;
  }

  if (editingRoute.value.candidates.some((candidate) => !candidate.model_id)) {
    toastController.error($t("modelRoutePage.alert.candidateModelRequired"));
    return false;
  }

  const seen = new Set<string>();
  for (const candidate of editingRoute.value.candidates) {
    if (!candidate.model_id) continue;
    if (seen.has(candidate.model_id)) {
      toastController.error($t("modelRoutePage.alert.duplicateCandidate"));
      return false;
    }
    seen.add(candidate.model_id);
  }

  return true;
};

const handleSaveRoute = async () => {
  if (!validateEditingRoute()) {
    return;
  }

  const payload: ModelRoutePayload = {
    route_name: editingRoute.value.route_name.trim(),
    description: editingRoute.value.description.trim() || null,
    is_enabled: editingRoute.value.is_enabled,
    expose_in_models: editingRoute.value.expose_in_models,
    candidates: buildCandidatePayload(),
  };

  try {
    if (editingRoute.value.id) {
      const updatePayload: ModelRouteUpdatePayload = { ...payload };
      await Api.updateModelRoute(editingRoute.value.id, updatePayload);
    } else {
      await Api.createModelRoute(payload);
    }
    showEditModal.value = false;
    await fetchRouteList();
  } catch (err: unknown) {
    console.error("Failed to save model route:", err);
    toastController.error(
      $t("modelRoutePage.alert.saveFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
};

const handleToggleEnabled = async (
  item: ModelRouteListItem,
  isEnabled: boolean,
) => {
  try {
    await Api.updateModelRoute(item.route.id, { is_enabled: isEnabled });
    await fetchRouteList();
  } catch (err: unknown) {
    console.error("Failed to toggle model route enabled state:", err);
    toastController.error(
      $t("modelRoutePage.alert.toggleFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
};

const handleToggleExpose = async (
  item: ModelRouteListItem,
  exposeInModels: boolean,
) => {
  try {
    await Api.updateModelRoute(item.route.id, {
      expose_in_models: exposeInModels,
    });
    await fetchRouteList();
  } catch (err: unknown) {
    console.error("Failed to toggle model route exposure:", err);
    toastController.error(
      $t("modelRoutePage.alert.toggleExposeFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
};

const handleDeleteRoute = async (id: number, name: string) => {
  if (
    !(await confirm({
      title: $t("modelRoutePage.confirmDelete", { name }),
    }))
  ) {
    return;
  }

  try {
    await Api.deleteModelRoute(id);
    await fetchRouteList();
  } catch (err: unknown) {
    console.error("Failed to delete model route:", err);
    toastController.error(
      $t("modelRoutePage.alert.deleteFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
};
</script>
