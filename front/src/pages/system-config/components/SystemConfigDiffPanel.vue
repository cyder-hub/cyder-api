<script setup lang="ts">
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import type {
  SystemConfigPreviewResponse,
  SystemConfigValidationIssue,
} from "@/services/types";

defineProps<{
  preview: SystemConfigPreviewResponse;
  diffRows: Array<{
    path: string;
    oldText: string;
    newText: string;
  }>;
  warningRows: SystemConfigValidationIssue[];
  runtimeActionLabels: string[];
  writeDisabledReasonLabel: (reason: string) => string;
}>();
</script>

<template>
  <div class="space-y-4 rounded-lg border border-gray-200 bg-white p-4">
    <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <h3 class="text-base font-semibold text-gray-900">
          {{ $t("systemConfigPage.preview.title") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{
            $t("systemConfigPage.preview.diffSummary", {
              count: preview.diff.length,
            })
          }}
        </p>
      </div>
      <Badge
        variant="outline"
        :class="
          preview.validation.valid
            ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
            : 'border-red-200 bg-red-50 text-red-700'
        "
      >
        {{
          preview.validation.valid
            ? $t("systemConfigPage.preview.valid")
            : $t("systemConfigPage.preview.invalid")
        }}
      </Badge>
    </div>

    <div v-if="preview.validation.errors.length" class="space-y-1">
      <p
        v-for="issue in preview.validation.errors"
        :key="`${issue.path}-${issue.code}`"
        class="break-words text-sm text-red-600"
      >
        {{ issue.path }}: {{ issue.message }}
      </p>
    </div>

    <div v-if="warningRows.length || preview.write_disabled_reason" class="space-y-1 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2">
      <p
        v-if="preview.write_disabled_reason"
        class="break-words text-sm text-amber-800"
      >
        {{ writeDisabledReasonLabel(preview.write_disabled_reason) }}
      </p>
      <p
        v-for="issue in warningRows"
        :key="`warning-${issue.path}-${issue.code}`"
        class="break-words text-sm text-amber-800"
      >
        {{ issue.path }}: {{ issue.message }}
      </p>
    </div>

    <div v-if="runtimeActionLabels.length" class="flex flex-wrap gap-1.5">
      <Badge
        v-for="label in runtimeActionLabels"
        :key="label"
        variant="outline"
        class="border-gray-200 bg-gray-50 text-gray-700"
      >
        {{ label }}
      </Badge>
    </div>

    <div v-if="diffRows.length" class="overflow-hidden rounded-lg border border-gray-200">
      <div class="grid grid-cols-1 divide-y divide-gray-100 md:grid-cols-3 md:divide-x md:divide-y-0">
        <div
          v-for="diff in diffRows"
          :key="diff.path"
          class="contents"
        >
          <div class="px-3 py-2 font-mono text-xs font-medium text-gray-900">
            {{ diff.path }}
          </div>
          <pre class="max-h-28 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-500">{{ diff.oldText }}</pre>
          <pre class="max-h-28 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-900">{{ diff.newText }}</pre>
        </div>
      </div>
    </div>
    <p v-else class="text-sm text-gray-500">
      {{ $t("systemConfigPage.preview.noChanges") }}
    </p>

    <div>
      <Label>{{ $t("systemConfigPage.preview.nextOverride") }}</Label>
      <pre class="mt-2 max-h-56 overflow-auto rounded-md bg-gray-950 px-3 py-2 font-mono text-xs text-gray-100">{{ preview.next_override_yaml }}</pre>
    </div>
  </div>
</template>
