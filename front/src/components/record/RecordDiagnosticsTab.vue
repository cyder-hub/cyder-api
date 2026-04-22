<template>
  <div class="space-y-5 text-sm">
    <RecordArtifactStatePanel
      v-if="loading"
      :title="$t('recordPage.detailDialog.diagnostics.loading')"
      loading
    />

    <RecordArtifactStatePanel
      v-else-if="error"
      :title="$t('recordPage.detailDialog.diagnostics.failed')"
      :message="error"
      tone="danger"
      retryable
      @retry="$emit('reload')"
    />

    <template v-else-if="artifacts">
      <section>
        <div class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between">
          <h3 class="text-base font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.diagnostics.requestSnapshot") }}
          </h3>
          <Badge v-if="artifacts.request_snapshot" variant="outline" class="w-fit font-mono text-xs">
            {{ artifacts.request_snapshot.operation_kind }}
          </Badge>
        </div>
        <div
          v-if="!artifacts.request_snapshot"
          class="mt-3 rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.diagnostics.noRequestSnapshot") }}
        </div>
        <div v-else class="mt-3 space-y-3">
          <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2">
            <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ $t("recordPage.detailDialog.diagnostics.labels.path") }}
              </dt>
              <dd class="break-all text-right font-mono text-xs text-gray-900">
                {{ artifacts.request_snapshot.request_path }}
              </dd>
            </div>
            <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ $t("recordPage.detailDialog.diagnostics.labels.operation") }}
              </dt>
              <dd class="text-right text-sm text-gray-900">
                {{ artifacts.request_snapshot.operation_kind }}
              </dd>
            </div>
          </dl>
          <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <NameValueBlock
              :title="$t('recordPage.detailDialog.diagnostics.queryParams')"
              :items="artifacts.request_snapshot.query_params"
            />
            <NameValueBlock
              :title="$t('recordPage.detailDialog.diagnostics.sanitizedOriginalHeaders')"
              :items="artifacts.request_snapshot.sanitized_original_headers"
            />
          </div>
        </div>
      </section>

      <section>
        <div class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between">
          <h3 class="text-base font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.diagnostics.candidateManifest") }}
          </h3>
          <Badge variant="outline" class="w-fit font-mono text-xs">
            {{ $t("recordPage.detailDialog.diagnostics.candidateCount", { count: artifacts.candidate_manifest.items.length }) }}
          </Badge>
        </div>
        <div
          v-if="!artifacts.candidate_manifest.has_asset"
          class="mt-3 rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.diagnostics.noCandidateManifest") }}
        </div>
        <div v-else class="mt-3 overflow-hidden border-y border-gray-100">
          <div
            v-for="candidate in artifacts.candidate_manifest.items"
            :key="`${candidate.candidate_position}-${candidate.provider_id}-${candidate.model_id}`"
            class="grid grid-cols-1 gap-2 border-t border-gray-100 px-4 py-3 first:border-t-0 lg:grid-cols-[auto_minmax(0,1fr)_minmax(0,1fr)_auto]"
          >
            <div class="font-mono text-xs text-gray-500">
              #{{ candidate.candidate_position }}
            </div>
            <div class="min-w-0">
              <div class="truncate text-sm font-medium text-gray-900">
                {{ formatRouteName(candidate.route_name, candidate.route_id) }}
              </div>
              <div class="mt-1 truncate font-mono text-xs text-gray-500">
                {{ candidate.provider_key }} / {{ candidate.provider_api_key_mode }}
              </div>
            </div>
            <div class="min-w-0">
              <div class="truncate font-mono text-xs text-gray-900">
                {{ candidate.model_name }}
              </div>
              <div class="mt-1 truncate font-mono text-xs text-gray-500">
                {{ candidate.real_model_name || "/" }}
              </div>
            </div>
            <Badge variant="outline" class="w-fit font-mono text-[11px]">
              {{ candidate.llm_api_type }}
            </Badge>
          </div>
        </div>
      </section>

      <section>
        <div class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between">
          <h3 class="text-base font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.diagnostics.transformDiagnostics") }}
          </h3>
          <div class="flex flex-wrap items-center gap-2">
            <Badge variant="outline" class="font-mono text-xs">
              {{ $t("recordPage.detailDialog.diagnostics.itemCount", { count: artifacts.transform_diagnostics.summary.count }) }}
            </Badge>
            <Badge
              v-if="artifacts.transform_diagnostics.summary.max_loss_level"
              variant="outline"
              class="font-mono text-xs"
            >
              {{ formatLossLevel(artifacts.transform_diagnostics.summary.max_loss_level) }}
            </Badge>
          </div>
        </div>
        <div
          v-if="!artifacts.transform_diagnostics.has_asset"
          class="mt-3 rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.diagnostics.noTransformDiagnostics") }}
        </div>
        <div v-else class="mt-3 space-y-4">
          <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-3">
            <div class="border-b border-gray-100 py-2.5">
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ $t("recordPage.detailDialog.diagnostics.labels.kinds") }}
              </dt>
              <dd class="mt-1 text-sm text-gray-900">
                {{ artifacts.transform_diagnostics.summary.kinds.join(", ") || "/" }}
              </dd>
            </div>
            <div class="border-b border-gray-100 py-2.5">
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ $t("recordPage.detailDialog.diagnostics.labels.phases") }}
              </dt>
              <dd class="mt-1 text-sm text-gray-900">
                {{ artifacts.transform_diagnostics.summary.phases.join(", ") || "/" }}
              </dd>
            </div>
            <div class="border-b border-gray-100 py-2.5">
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ $t("recordPage.detailDialog.diagnostics.labels.maxLoss") }}
              </dt>
              <dd class="mt-1 text-sm text-gray-900">
                {{ formatLossLevel(artifacts.transform_diagnostics.summary.max_loss_level) }}
              </dd>
            </div>
          </dl>

          <div class="space-y-3">
            <details
              v-for="(item, index) in artifacts.transform_diagnostics.items"
              :key="`${item.phase}-${index}`"
              class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2"
            >
              <summary class="cursor-pointer text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ item.phase }}
              </summary>
              <pre class="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700">{{ formatJsonText(item.diagnostic) }}</pre>
            </details>
          </div>
        </div>
      </section>
    </template>

    <RecordArtifactStatePanel
      v-else
      :title="$t('recordPage.detailDialog.diagnostics.lazyEmpty')"
    />
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Badge } from "@/components/ui/badge";
import NameValueBlock from "./NameValueBlock.vue";
import RecordArtifactStatePanel from "./RecordArtifactStatePanel.vue";
import type { RecordArtifactResponse } from "@/store/types";
import { formatJsonText, formatLossLevel } from "./recordFormat";

defineEmits<{
  reload: [];
}>();

defineProps<{
  artifacts: RecordArtifactResponse | null;
  loading: boolean;
  error: string | null;
}>();

const { t: $t } = useI18n();

const formatRouteName = (routeName: string | null, routeId: number | null) =>
  routeName || $t("recordPage.detailDialog.diagnostics.routeFallback", { id: routeId ?? "/" });
</script>
