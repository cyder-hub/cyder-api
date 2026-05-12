<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader
      :title="t('providerEditPage.requestPatch.title')"
      :help="t('providerEditPage.requestPatch.description')"
      :help-label="t('providerEditPage.requestPatch.title')"
    >
      <template #actions>
      <Badge variant="outline" class="w-fit font-mono text-[11px]">
        {{ editingData.request_patches.length }}
      </Badge>
      </template>
    </SectionHeader>

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
import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import type { EditingProviderData } from "../types";
import { useProviderRequestPatch } from "../composables/useProviderRequestPatch";

const editingData = defineModel<EditingProviderData>("editingData", {
  required: true,
});

const { t } = useI18n();
const { actions, refreshRules } = useProviderRequestPatch(editingData);
</script>
