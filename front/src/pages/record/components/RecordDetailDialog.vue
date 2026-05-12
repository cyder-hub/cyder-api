<template>
  <Dialog v-model:open="isOpen">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-[92vw] xl:max-w-6xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle class="pr-8 text-base font-semibold text-gray-900 sm:text-lg">
          {{ $t("recordPage.detailDialog.title") }}
        </DialogTitle>
      </DialogHeader>

      <div class="min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-6">
        <div v-if="loading" class="py-10 text-center text-gray-500">
          <div class="mb-2 inline-block h-8 w-8 animate-spin rounded-full border-b-2 border-gray-900"></div>
          <div>{{ $t("recordPage.detailDialog.loading") }}</div>
        </div>

        <div v-else-if="record" class="space-y-4">
          <section class="border-b border-gray-100 pb-1">
            <dl class="grid grid-cols-1 divide-y divide-gray-100 sm:grid-cols-2 sm:divide-x sm:divide-y-0 xl:grid-cols-5">
              <div class="flex items-center justify-between gap-3 px-4 py-3">
                <dt class="text-xs uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.detailDialog.summary.status") }}
                </dt>
                <dd>
                  <Badge :variant="getStatusBadgeVariant(record.overall_status)">
                    {{ record.overall_status || "/" }}
                  </Badge>
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3 px-4 py-3">
                <dt class="text-xs uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.detailDialog.summary.attempts") }}
                </dt>
                <dd class="font-mono text-sm font-semibold text-gray-900">
                  {{ record.attempt_count }} / {{ record.retry_count }} / {{ record.fallback_count }}
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3 px-4 py-3">
                <dt class="text-xs uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.detailDialog.summary.provider") }}
                </dt>
                <dd class="truncate text-right text-sm font-medium text-gray-900">
                  {{ providerName }}
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3 px-4 py-3">
                <dt class="text-xs uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.detailDialog.summary.model") }}
                </dt>
                <dd class="truncate text-right font-mono text-xs text-gray-900">
                  {{ record.requested_model_name || record.final_model_name_snapshot || "/" }}
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3 px-4 py-3">
                <dt class="text-xs uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.detailDialog.summary.diagnostics") }}
                </dt>
                <dd>
                  <Badge
                    :variant="record.has_transform_diagnostics ? 'outline' : 'secondary'"
                    class="font-mono text-xs"
                  >
                    {{ record.transform_diagnostic_count }}
                  </Badge>
                </dd>
              </div>
            </dl>
          </section>

          <div class="app-scroll-x border-b border-gray-100">
            <div class="flex min-w-max gap-1">
              <button
                v-for="tab in tabs"
                :key="tab.value"
                type="button"
                class="border-b-2 px-3 py-2 text-sm font-medium transition-colors"
                :class="
                  activeTab === tab.value
                    ? 'border-gray-900 text-gray-900'
                    : 'border-transparent text-gray-500 hover:text-gray-900'
                "
                @click="activeTab = tab.value"
              >
                {{ $t(tab.labelKey) }}
              </button>
            </div>
          </div>

          <RecordOverviewTab
            v-if="activeTab === 'overview'"
            :record="record"
            :api-key-name="apiKeyName"
            :provider-name="providerName"
          />
          <RecordAttemptsTab
            v-else-if="activeTab === 'attempts'"
            :attempts="attempts"
          />
          <RecordDiagnosticsPanel
            v-else-if="activeTab === 'diagnostics'"
            :artifacts="artifacts"
            :loading="artifactsLoading"
            :error="artifactsError"
            @reload="$emit('reloadArtifacts')"
          />
          <section v-else-if="activeTab === 'payloads'" class="space-y-4">
            <div class="border-b border-gray-100 pb-2">
              <h3 class="text-base font-semibold text-gray-900">
                {{ $t("recordPage.detailDialog.payloads.title") }}
              </h3>
              <p class="mt-1 text-xs leading-5 text-gray-500">
                {{ $t("recordPage.detailDialog.payloads.description") }}
              </p>
            </div>
            <template v-if="record.bundle_storage_type">
              <RecordBodyViewer
                v-if="shouldRenderPayloadViewer(activeTab, record.bundle_storage_type)"
                :record-id="record.id"
                :storage-type="record.bundle_storage_type"
                :status="record.overall_status"
                :attempts="attempts"
              />
            </template>
            <div
              v-else
              class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
            >
              {{ $t("recordPage.detailDialog.payloads.empty") }}
            </div>
          </section>
          <RecordReplayPanel
            v-else
            :record-id="record.id"
            :attempts="attempts"
            :artifacts="artifacts"
            :loading="artifactsLoading"
            :error="artifactsError"
            :selected-attempt-id="selectedAttemptId"
            :selected-replay-run-id="selectedReplayRunId"
            @reload="$emit('reloadArtifacts')"
            @update:selected-attempt-id="$emit('update:selectedAttemptId', $event)"
            @update:selected-replay-run-id="$emit('update:selectedReplayRunId', $event)"
          />
        </div>

        <div
          v-else
          class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.noRecord") }}
        </div>
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button variant="secondary" class="w-full sm:w-auto" @click="isOpen = false">
          {{ $t("common.close") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import RecordBodyViewer from "./RecordBodyViewer.vue";
import RecordAttemptsTab from "./RecordAttemptsTab.vue";
import RecordDiagnosticsPanel from "./RecordDiagnosticsPanel.vue";
import RecordOverviewTab from "./RecordOverviewTab.vue";
import RecordReplayPanel from "./RecordReplayPanel.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { RecordArtifactResponse, RecordAttempt, RecordRequest } from "@/services/types";
import {
  RECORD_DETAIL_TABS,
  shouldRenderPayloadViewer,
  type RecordDetailTab,
} from "../composables/useRecordDetail";
import { getStatusBadgeVariant } from "../composables/recordFormat";

const props = defineProps<{
  open: boolean;
  loading: boolean;
  record: RecordRequest | null;
  attempts: RecordAttempt[];
  activeTab: RecordDetailTab;
  artifacts: RecordArtifactResponse | null;
  artifactsLoading: boolean;
  artifactsError: string | null;
  selectedAttemptId: number | null;
  selectedReplayRunId: number | null;
  apiKeyName: string;
  providerName: string;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  "update:activeTab": [value: RecordDetailTab];
  "reloadArtifacts": [];
  "update:selectedAttemptId": [value: number | null];
  "update:selectedReplayRunId": [value: number | null];
}>();

const { t: $t } = useI18n();

const isOpen = computed({
  get: () => props.open,
  set: (value: boolean) => emit("update:open", value),
});

const tabs = RECORD_DETAIL_TABS;

const activeTab = computed({
  get: () => props.activeTab,
  set: (value: RecordDetailTab) => emit("update:activeTab", value),
});
</script>
