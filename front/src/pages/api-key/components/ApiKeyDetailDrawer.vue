<script setup lang="ts">
import { useI18n } from "vue-i18n";
import {
  Copy,
  Eye,
  Loader2,
  Pencil,
  RotateCcw,
  Trash2,
} from "lucide-vue-next";

import { Button } from "@/components/ui/button";
import type { ApiKeyDetail, ApiKeyReveal, ApiKeyRuntimeSnapshot } from "@/services/types";
import { maskedApiKey } from "../composables/useApiKeyDetail";
import ApiKeyGovernancePanel from "./ApiKeyGovernancePanel.vue";

defineProps<{
  detail: ApiKeyDetail | null;
  runtime: ApiKeyRuntimeSnapshot;
  detailLoading: boolean;
  secretReveal: ApiKeyReveal | null;
  providerNameById: Map<number, string>;
  modelNameById: Map<number, string>;
  routeNameById: Map<number, string>;
}>();

defineEmits<{
  (event: "reveal", id: number): void;
  (event: "rotate", id: number): void;
  (event: "edit", id: number): void;
  (event: "delete", id: number): void;
  (event: "copySecret", secret: string): void;
  (event: "closeSecret"): void;
}>();

const { t } = useI18n();
</script>

<template>
  <div class="rounded-xl border border-gray-200 bg-white xl:flex xl:h-full xl:min-h-0 xl:flex-col">
    <div class="border-b border-gray-100 px-4 py-4 sm:px-5">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h2 class="text-base font-semibold text-gray-900">
            {{ detail ? detail.name : t("apiKeyPage.sections.detailTitle") }}
          </h2>
          <p class="mt-1 text-sm text-gray-500">
            {{
              detail
                ? maskedApiKey(detail)
                : t("apiKeyPage.sections.detailDescription")
            }}
          </p>
        </div>

        <div v-if="detail" class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
          <Button variant="outline" class="w-full sm:w-auto" @click="$emit('reveal', detail.id)">
            <Eye class="mr-1.5 h-4 w-4" />
            {{ t("apiKeyPage.actions.reveal") }}
          </Button>
          <Button variant="outline" class="w-full sm:w-auto" @click="$emit('rotate', detail.id)">
            <RotateCcw class="mr-1.5 h-4 w-4" />
            {{ t("apiKeyPage.actions.rotate") }}
          </Button>
          <Button variant="outline" class="w-full sm:w-auto" @click="$emit('edit', detail.id)">
            <Pencil class="mr-1.5 h-4 w-4" />
            {{ t("common.edit") }}
          </Button>
          <Button variant="destructive" class="w-full sm:w-auto" @click="$emit('delete', detail.id)">
            <Trash2 class="mr-1.5 h-4 w-4" />
            {{ t("common.delete") }}
          </Button>
        </div>
      </div>
    </div>

    <div v-if="detailLoading" class="flex items-center justify-center px-4 py-16 xl:flex-1">
      <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
      <span class="text-sm text-gray-500">
        {{ t("apiKeyPage.loadingDetail") }}
      </span>
    </div>

    <div v-else-if="!detail" class="px-4 py-16 text-center text-sm text-gray-500 xl:flex-1">
      {{ t("apiKeyPage.noSelection") }}
    </div>

    <div v-else class="space-y-6 px-4 py-4 sm:px-5 xl:min-h-0 xl:flex-1 xl:overflow-y-auto">
      <div
        v-if="secretReveal && secretReveal.id === detail.id"
        class="rounded-lg border border-gray-200 bg-gray-50 px-4 py-4"
      >
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h3 class="text-sm font-semibold text-gray-900">
              {{ t("apiKeyPage.secret.title") }}
            </h3>
            <p class="mt-1 text-sm text-gray-500">
              {{ t("apiKeyPage.secret.description") }}
            </p>
          </div>
          <div class="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              class="text-xs"
              @click="$emit('copySecret', secretReveal.api_key)"
            >
              <Copy class="mr-1 h-3.5 w-3.5" />
              {{ t("apiKeyPage.actions.copySecret") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="text-xs text-gray-500"
              @click="$emit('closeSecret')"
            >
              {{ t("common.close") }}
            </Button>
          </div>
        </div>
        <textarea
          readonly
          rows="3"
          class="mt-3 flex w-full rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none"
          :value="secretReveal.api_key"
        />
      </div>

      <ApiKeyGovernancePanel
        :detail="detail"
        :runtime="runtime"
        :provider-name-by-id="providerNameById"
        :model-name-by-id="modelNameById"
        :route-name-by-id="routeNameById"
      />
    </div>
  </div>
</template>
