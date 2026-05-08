<template>
  <section class="space-y-4">
    <div
      class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
    >
      <div>
        <h3 class="text-base font-semibold text-gray-900">
          {{ $t("recordPage.detailDialog.replay.historyTitle") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("recordPage.detailDialog.replay.historyDescription") }}
        </p>
      </div>
      <Button
        variant="outline"
        class="w-full sm:w-auto"
        :disabled="historyLoading || historySelectionLoading"
        @click="$emit('reload')"
      >
        {{
          historyLoading
            ? $t("recordPage.detailDialog.replay.actions.loadingHistory")
            : $t("recordPage.detailDialog.replay.actions.reloadHistory")
        }}
      </Button>
    </div>

    <div
      v-if="historyLoading && replayRuns.length === 0"
      class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
    >
      {{ $t("recordPage.detailDialog.replay.historyLoading") }}
    </div>

    <div
      v-else-if="historyError"
      class="space-y-3 rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-700"
    >
      <div>{{ historyError }}</div>
      <Button
        variant="outline"
        class="w-full border-red-200 bg-white text-red-700 hover:bg-red-50 sm:w-auto"
        @click="$emit('reload')"
      >
        {{ $t("recordPage.detailDialog.replay.actions.retryHistory") }}
      </Button>
    </div>

    <div
      v-else-if="replayRuns.length === 0"
      class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
    >
      {{ $t("recordPage.detailDialog.replay.historyEmpty") }}
    </div>

    <div v-else class="overflow-hidden rounded-lg border border-gray-200 bg-white">
      <button
        v-for="replayRun in replayRuns"
        :key="replayRun.id"
        type="button"
        class="w-full border-b border-gray-100 px-4 py-3 text-left transition-colors last:border-b-0"
        :class="
          selectedReplayRunId === replayRun.id ? 'bg-gray-50' : 'hover:bg-gray-50/70'
        "
        :disabled="historySelectionLoading"
        @click="$emit('open', replayRun.id)"
      >
        <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0 space-y-2">
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline" class="font-mono text-xs">
                #{{ replayRun.id }}
              </Badge>
              <Badge
                :variant="getStatusBadgeVariant(replayRun.status)"
                class="font-mono text-xs"
              >
                {{ replayRun.status }}
              </Badge>
              <Badge variant="secondary" class="font-mono text-xs">
                {{ replayRun.replay_kind }}
              </Badge>
              <Badge variant="secondary" class="font-mono text-xs">
                {{ replayRun.replay_mode }}
              </Badge>
            </div>
            <div class="grid grid-cols-1 gap-1 text-xs text-gray-500 sm:grid-cols-2 xl:grid-cols-4">
              <div>
                {{ $t("recordPage.detailDialog.replay.labels.created") }}:
                <span class="font-mono text-gray-700">{{ formatDate(replayRun.created_at) }}</span>
              </div>
              <div>
                {{ $t("recordPage.detailDialog.replay.labels.route") }}:
                <span class="font-mono text-gray-700">{{ replayRun.executed_route_name || emptyValue }}</span>
              </div>
              <div>
                {{ $t("recordPage.detailDialog.replay.labels.http") }}:
                <span class="font-mono text-gray-700">{{ replayRun.http_status ?? emptyValue }}</span>
              </div>
              <div>
                {{ $t("recordPage.detailDialog.replay.labels.errorCode") }}:
                <span class="font-mono text-gray-700">{{ replayRun.error_code || emptyValue }}</span>
              </div>
            </div>
          </div>
          <div class="text-xs text-gray-500 sm:text-right">
            {{
              historySelectionLoading && selectedReplayRunId === replayRun.id
                ? $t("recordPage.detailDialog.replay.actions.openingHistory")
                : $t("recordPage.detailDialog.replay.actions.openHistory")
            }}
          </div>
        </div>
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { RecordReplayRun } from "@/services/types";
import {
  emptyValue,
  formatDate,
  getStatusBadgeVariant,
} from "../composables/recordFormat";

defineProps<{
  replayRuns: RecordReplayRun[];
  historyLoading: boolean;
  historySelectionLoading: boolean;
  historyError: string | null;
  selectedReplayRunId: number | null;
}>();

defineEmits<{
  reload: [];
  open: [id: number];
}>();
</script>
