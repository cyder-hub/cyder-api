<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <SectionHeader
        :title="$t('providerEditPage.sources.title')"
        :help="$t('providerEditPage.sources.description')"
        :help-label="$t('providerEditPage.sources.title')"
      />
      <Button
        v-if="editingData.id"
        class="w-full sm:w-auto"
        @click="openCreate"
      >
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.sources.add") }}
      </Button>
    </div>

    <div
      v-if="!editingData.id"
      class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm leading-6 text-gray-500"
    >
      {{ $t("providerEditPage.sources.createHint") }}
    </div>

    <div
      v-else-if="editingData.upstream_sources.length === 0"
      class="flex flex-col items-center justify-center rounded-lg border border-dashed border-amber-200 bg-amber-50/60 px-4 py-10 text-center"
    >
      <Network class="h-8 w-8 stroke-1 text-amber-600" />
      <p class="mt-3 text-sm font-medium text-amber-900">
        {{ $t("providerEditPage.sources.zeroWarning") }}
      </p>
      <p class="mt-1 max-w-md text-xs leading-5 text-amber-800/80">
        {{ $t("providerEditPage.sources.zeroDescription") }}
      </p>
      <Button variant="outline" class="mt-4" @click="openCreate">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.sources.add") }}
      </Button>
    </div>

    <template v-else>
      <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
        <Table>
          <TableHeader>
            <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("providerEditPage.sources.table.profile") }}
              </TableHead>
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("providerEditPage.sources.table.endpoint") }}
              </TableHead>
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("providerEditPage.sources.table.state") }}
              </TableHead>
              <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("common.actions") }}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-for="source in editingData.upstream_sources" :key="source.id">
              <TableCell>
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[10px]">
                    {{ source.profile_type }}
                  </Badge>
                  <span class="font-mono text-xs text-gray-500">#{{ source.id }}</span>
                </div>
              </TableCell>
              <TableCell class="max-w-[22rem]">
                <span class="block truncate font-mono text-xs text-gray-700" :title="source.endpoint">
                  {{ source.endpoint }}
                </span>
              </TableCell>
              <TableCell>
                <div class="flex flex-wrap gap-1.5">
                  <Badge :class="source.is_enabled ? enabledClass : disabledClass" class="font-mono text-[10px]">
                    {{ source.is_enabled ? $t("providerEditPage.sources.enabled") : $t("providerEditPage.sources.disabled") }}
                  </Badge>
                  <Badge v-if="source.is_default" class="border-gray-900 bg-gray-900 font-mono text-[10px] text-white hover:bg-gray-900">
                    {{ $t("providerEditPage.sources.default") }}
                  </Badge>
                  <Badge
                    v-if="sourceRuntime(source.id)"
                    :class="runtimeBadgeClass(sourceRuntime(source.id)?.runtime_level ?? 'no_traffic')"
                    class="font-mono text-[10px]"
                  >
                    {{ runtimeLevelLabel(sourceRuntime(source.id)?.runtime_level ?? "no_traffic") }}
                  </Badge>
                  <Badge v-else variant="outline" class="font-mono text-[10px] text-gray-500">
                    {{ $t("providerEditPage.sources.circuitUnavailable") }}
                  </Badge>
                </div>
                <p v-if="sourceRuntime(source.id)" class="mt-1 text-[11px] text-gray-500">
                  {{ $t("providerRuntimePage.metrics.failures") }}:
                  {{ sourceRuntime(source.id)?.consecutive_failures ?? 0 }}
                </p>
              </TableCell>
              <TableCell class="text-right">
                <div class="flex items-center justify-end gap-1">
                  <Button
                    variant="outline"
                    size="sm"
                    class="mr-1 text-gray-700"
                    :aria-label="$t('providerEditPage.sources.requestPatchAction')"
                    :disabled="isBusy(source.id)"
                    @click="emit('requestPatch', source.id)"
                  >
                    {{ $t("providerEditPage.sources.requestPatchAction") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="text-gray-600"
                    :aria-label="$t('providerEditPage.sources.checkAction')"
                    :disabled="isBusy(source.id)"
                    @click="emit('checkSource', source.id)"
                  >
                    <Check class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="text-gray-600"
                    :aria-label="$t('providerEditPage.sources.editAction')"
                    :disabled="isBusy(source.id)"
                    @click="openEdit(source)"
                  >
                    <Pencil class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="text-gray-600"
                    :aria-label="source.is_enabled ? $t('providerEditPage.sources.disableAction') : $t('providerEditPage.sources.enableAction')"
                    :disabled="isBusy(source.id)"
                    @click="toggleEnabled(source)"
                  >
                    <Power class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="text-gray-600"
                    :aria-label="source.is_default ? $t('providerEditPage.sources.unsetDefaultAction') : $t('providerEditPage.sources.setDefaultAction')"
                    :disabled="isBusy(source.id)"
                    @click="toggleDefault(source)"
                  >
                    <Star class="h-4 w-4" :class="source.is_default ? 'fill-current' : ''" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="text-gray-400 hover:text-red-600"
                    :aria-label="$t('providerEditPage.sources.deleteAction')"
                    :disabled="isBusy(source.id)"
                    @click="deleteSource(source)"
                  >
                    <Trash2 class="h-4 w-4" />
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>

      <div class="grid grid-cols-1 gap-3 md:hidden">
        <MobileCrudCard
          v-for="source in editingData.upstream_sources"
          :key="source.id"
          :title="source.profile_type"
          :description="`#${source.id}`"
        >
          <template #header>
            <div class="flex flex-wrap gap-1.5">
              <Badge :class="source.is_enabled ? enabledClass : disabledClass" class="font-mono text-[10px]">
                {{ source.is_enabled ? $t("providerEditPage.sources.enabled") : $t("providerEditPage.sources.disabled") }}
              </Badge>
              <Badge v-if="source.is_default" class="border-gray-900 bg-gray-900 font-mono text-[10px] text-white hover:bg-gray-900">
                {{ $t("providerEditPage.sources.default") }}
              </Badge>
              <Badge
                v-if="sourceRuntime(source.id)"
                :class="runtimeBadgeClass(sourceRuntime(source.id)?.runtime_level ?? 'no_traffic')"
                class="font-mono text-[10px]"
              >
                {{ runtimeLevelLabel(sourceRuntime(source.id)?.runtime_level ?? "no_traffic") }}
              </Badge>
              <Badge v-else variant="outline" class="font-mono text-[10px] text-gray-500">
                {{ $t("providerEditPage.sources.circuitUnavailable") }}
              </Badge>
            </div>
            <p v-if="sourceRuntime(source.id)" class="mt-1 text-[11px] text-gray-500">
              {{ $t("providerRuntimePage.metrics.failures") }}:
              {{ sourceRuntime(source.id)?.consecutive_failures ?? 0 }}
            </p>
          </template>
          <div class="space-y-2 text-xs text-gray-500">
            <div class="rounded-lg border border-gray-100 px-3 py-2.5">
              <span class="text-[11px] uppercase tracking-wide text-gray-400">
                {{ $t("providerEditPage.sources.table.endpoint") }}
              </span>
              <p class="mt-1 break-all font-mono text-gray-700">{{ source.endpoint }}</p>
            </div>
            <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("providerEditPage.sources.table.profile") }}</span>
              <span class="font-mono text-gray-700">{{ source.profile_type }} · #{{ source.id }}</span>
            </div>
          </div>
          <template #actions>
            <Button variant="ghost" size="sm" class="w-full justify-center text-gray-600" :disabled="isBusy(source.id)" @click="emit('checkSource', source.id)">
              <Check class="mr-1.5 h-3.5 w-3.5" />
              {{ $t("providerEditPage.sources.checkAction") }}
            </Button>
            <Button variant="ghost" size="sm" class="w-full justify-center" :disabled="isBusy(source.id)" @click="openEdit(source)">
              <Pencil class="mr-1.5 h-3.5 w-3.5" />
              {{ $t("common.edit") }}
            </Button>
            <Button variant="outline" size="sm" class="w-full justify-center" :disabled="isBusy(source.id)" @click="emit('requestPatch', source.id)">
              {{ $t("providerEditPage.sources.requestPatchAction") }}
            </Button>
            <Button variant="ghost" size="sm" class="w-full justify-center text-gray-600" :disabled="isBusy(source.id)" @click="toggleDefault(source)">
              <Star class="mr-1.5 h-3.5 w-3.5" :class="source.is_default ? 'fill-current' : ''" />
              {{ source.is_default ? $t("providerEditPage.sources.unsetDefaultAction") : $t("providerEditPage.sources.setDefaultAction") }}
            </Button>
            <Button variant="ghost" size="sm" class="w-full justify-center text-gray-400 hover:text-red-600" :disabled="isBusy(source.id)" @click="deleteSource(source)">
              <Trash2 class="mr-1.5 h-3.5 w-3.5" />
              {{ $t("common.delete") }}
            </Button>
          </template>
        </MobileCrudCard>
      </div>
    </template>
  </section>

  <Drawer
    v-model:open="isDrawerOpen"
    :direction="isDesktop ? 'right' : 'bottom'"
    @update:open="(open) => !open && closeDrawer()"
  >
    <DrawerContent class="flex max-h-[92dvh] flex-col border-gray-200 bg-white p-0 outline-none md:h-full md:max-h-full md:max-w-xl md:rounded-none">
      <DrawerHeader class="border-b border-gray-100 px-4 py-4 text-left sm:px-6">
        <DrawerTitle class="text-lg font-semibold text-gray-900">
          {{ isEditing ? $t("providerEditPage.sources.editTitle") : $t("providerEditPage.sources.addTitle") }}
        </DrawerTitle>
        <DrawerDescription class="mt-1 text-sm leading-6 text-gray-500">
          {{ $t("providerEditPage.sources.drawerDescription") }}
        </DrawerDescription>
      </DrawerHeader>

      <div class="min-h-0 flex-1 space-y-5 overflow-y-auto px-4 py-5 sm:px-6">
        <div v-if="isEditing" class="space-y-1.5">
          <Label class="text-gray-700">{{ $t("providerEditPage.sources.profile") }}</Label>
          <div class="flex items-center gap-2 rounded-md border border-gray-200 bg-gray-50 px-3 py-2">
            <Badge variant="outline" class="font-mono text-[10px]">{{ draft.profile_type }}</Badge>
            <span class="font-mono text-xs text-gray-500">#{{ editingSourceId }}</span>
          </div>
        </div>
        <div v-else class="space-y-1.5">
          <Label class="text-gray-700">{{ $t("providerEditPage.sources.profile") }}</Label>
          <Select v-model="draft.profile_type">
            <SelectTrigger class="w-full">
              <SelectValue :placeholder="$t('providerEditPage.sources.profilePlaceholder')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="profile in providerProfileTypes" :key="profile" :value="profile">
                {{ profile }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="space-y-1.5">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.sources.endpoint") }}
            <span class="ml-0.5 text-red-500">*</span>
          </Label>
          <Input v-model="draft.endpoint" class="font-mono text-sm" />
        </div>

        <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
          <Label for="source_use_proxy" class="cursor-pointer text-gray-700">
            {{ $t("providerEditPage.labelUseProxy") }}
          </Label>
          <Checkbox id="source_use_proxy" v-model="draft.use_proxy" />
        </div>
        <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
          <Label for="source_enabled" class="cursor-pointer text-gray-700">
            {{ $t("providerEditPage.sources.enabled") }}
          </Label>
          <Checkbox id="source_enabled" v-model="draft.is_enabled" />
        </div>
        <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
          <div>
            <Label for="source_default" class="cursor-pointer text-gray-700">
              {{ $t("providerEditPage.sources.default") }}
            </Label>
            <p class="mt-1 text-xs leading-5 text-gray-500">
              {{ $t("providerEditPage.sources.defaultHint") }}
            </p>
          </div>
          <Checkbox id="source_default" v-model="draft.is_default" />
        </div>
      </div>

      <DrawerFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" :disabled="isSaving" @click="closeDrawer">
          {{ $t("common.cancel") }}
        </Button>
        <Button class="w-full sm:w-auto" :disabled="isSaving" @click="save">
          <Loader2 v-if="isSaving" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ isEditing ? $t("common.save") : $t("providerEditPage.sources.add") }}
        </Button>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useMediaQuery } from "@vueuse/core";
import { Check, Loader2, Network, Pencil, Plus, Power, Star, Trash2 } from "lucide-vue-next";

import MobileCrudCard from "@/components/MobileCrudCard.vue";
import SectionHeader from "@/components/SectionHeader.vue";
import * as providerRuntimeService from "@/services/providerRuntime";
import type { ProviderRuntimeItem, ProviderRuntimeLevel } from "@/services/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
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
import type { EditingProviderData } from "../types";
import { providerProfileTypes, useProviderSources } from "../composables/useProviderSources";

const { t: $t } = useI18n();
const isDesktop = useMediaQuery("(min-width: 768px)");
const editingData = defineModel<EditingProviderData>("editingData", { required: true });
const emit = defineEmits<{
  checkSource: [sourceId: number];
  requestPatch: [sourceId: number];
}>();
const {
  draft,
  editingSourceId,
  isDrawerOpen,
  isEditing,
  isSaving,
  busySourceId,
  closeDrawer,
  deleteSource,
  openCreate,
  openEdit,
  save,
  toggleDefault,
  toggleEnabled,
} = useProviderSources(editingData);

const enabledClass = computed(() => "border-emerald-200 bg-emerald-50 text-emerald-700");
const disabledClass = computed(() => "border-gray-200 bg-gray-100 text-gray-500");
const isBusy = (sourceId: number) => busySourceId.value === sourceId;

const runtimeBySourceId = ref<Record<number, ProviderRuntimeItem>>({});

const sourceRuntime = (sourceId: number) => runtimeBySourceId.value[sourceId];

const runtimeLevelLabel = (level: ProviderRuntimeLevel) =>
  $t(`providerRuntimePage.status.${level}`);

const runtimeBadgeClass = (level: ProviderRuntimeLevel) => {
  switch (level) {
    case "open":
      return "border-red-200 bg-red-50 text-red-700";
    case "half_open":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "degraded":
      return "border-orange-200 bg-orange-50 text-orange-700";
    case "healthy":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "no_traffic":
      return "border-gray-200 bg-gray-100 text-gray-600";
  }
};

const loadSourceRuntime = async () => {
  const providerId = editingData.value.id;
  if (!providerId) {
    runtimeBySourceId.value = {};
    return;
  }

  try {
    const { items } = await providerRuntimeService.getProviderRuntimeSnapshot({
      window: "1h",
      only_enabled: false,
    });
    runtimeBySourceId.value = Object.fromEntries(
      items
        .filter((item) => item.provider_id === providerId)
        .map((item) => [item.source_id, item]),
    );
  } catch {
    runtimeBySourceId.value = {};
  }
};

watch(
  () => [
    editingData.value.id,
    editingData.value.upstream_sources
      .map((source) => `${source.id}:${source.is_enabled}:${source.is_default}`)
      .join(","),
  ],
  () => {
    void loadSourceRuntime();
  },
  { immediate: true },
);
</script>
