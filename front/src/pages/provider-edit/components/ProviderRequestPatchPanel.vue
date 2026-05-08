<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <h3 class="text-lg font-semibold text-gray-900">
          {{ t("providerEditPage.requestPatch.title") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ t("providerEditPage.requestPatch.description") }}
        </p>
      </div>
      <Badge variant="outline" class="w-fit font-mono text-[11px]">
        {{ editingData.request_patches.length }}
      </Badge>
    </div>

    <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-4 py-3 text-sm text-gray-500">
      {{ t("providerEditPage.requestPatch.messages.providerDirect") }}
    </div>

    <RequestPatchRulesPanel
      owner-kind="provider"
      :owner-ready="!!editingData.id"
      :rules="editingData.request_patches"
      :actions="actions"
      @changed="refreshRules"
    />
  </section>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";

import RequestPatchRulesPanel from "@/components/request-patch/RequestPatchRulesPanel.vue";
import { Badge } from "@/components/ui/badge";
import type { EditingProviderData } from "../types";
import { useProviderRequestPatch } from "../composables/useProviderRequestPatch";

const editingData = defineModel<EditingProviderData>("editingData", {
  required: true,
});

const { t } = useI18n();
const { actions, refreshRules } = useProviderRequestPatch(editingData);
</script>
