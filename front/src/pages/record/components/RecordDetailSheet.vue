<template>
  <Drawer v-model:open="isOpen" direction="right">
    <DrawerContent class="flex flex-col p-0 outline-none">
      <DrawerHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DrawerTitle class="pr-8 text-base font-semibold text-gray-900 sm:text-lg">
          {{ $t("recordPage.detailDialog.title") }}
        </DrawerTitle>
      </DrawerHeader>

      <div class="min-h-0 flex-1 overflow-y-auto px-4 py-5 sm:px-6">
        <div v-if="loading" class="py-10 text-center text-sm text-gray-500">
          {{ $t("recordPage.detailDialog.loading") }}
        </div>

        <div v-else-if="record" class="space-y-6">
          <div class="flex flex-wrap items-center gap-3 border-b border-gray-100 pb-4">
            <Badge :variant="getStatusBadgeVariant(record.overall_status)">
              {{ record.overall_status }}
            </Badge>
            <span class="font-mono text-xs text-gray-500">#{{ record.id }}</span>
            <span v-if="record.upstream_http_status" class="font-mono text-xs text-gray-500">
              HTTP {{ record.upstream_http_status }}
            </span>
          </div>

          <section>
            <h3 class="mb-3 text-sm font-semibold text-gray-900">
              {{ $t("recordPage.detailDialog.tabs.overview") }}
            </h3>
            <dl class="grid grid-cols-1 gap-x-6 gap-y-4 sm:grid-cols-2">
              <div v-for="item in overviewItems" :key="item.label" class="min-w-0">
                <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
                <dd class="mt-1 break-words text-sm text-gray-900" :class="item.mono && 'font-mono text-xs'">
                  {{ item.value }}
                </dd>
              </div>
            </dl>
          </section>

          <section v-if="record.final_error_code || record.final_error_message" class="border-t border-gray-100 pt-5">
            <h3 class="mb-3 text-sm font-semibold text-gray-900">Error</h3>
            <div class="rounded-lg border border-red-100 bg-red-50 p-3 text-sm text-red-800">
              <p v-if="record.final_error_code" class="font-mono text-xs">{{ record.final_error_code }}</p>
              <p v-if="record.final_error_message" class="mt-1 whitespace-pre-wrap break-words">{{ record.final_error_message }}</p>
            </div>
          </section>
        </div>

        <div v-else class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500">
          {{ $t("recordPage.detailDialog.noRecord") }}
        </div>
      </div>

      <DrawerFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button variant="secondary" class="w-full sm:w-auto" @click="isOpen = false">
          {{ $t("common.close") }}
        </Button>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Drawer,
  DrawerContent,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import type { RecordRequest } from "@/services/types";
import type { RecordDetailTab } from "../composables/useRecordDetail";
import { emptyValue, formatDate, formatDuration, formatPrice, getStatusBadgeVariant } from "../composables/recordFormat";

const props = defineProps<{
  open: boolean;
  loading: boolean;
  record: RecordRequest | null;
  activeTab: RecordDetailTab;
  apiKeyName: string;
  providerName: string;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  "update:activeTab": [value: RecordDetailTab];
}>();

const { t: $t } = useI18n();
const isOpen = computed({
  get: () => props.open,
  set: (value: boolean) => emit("update:open", value),
});

const value = (input: string | number | null | undefined) =>
  input == null || input === "" ? emptyValue : String(input);

const overviewItems = computed(() => {
  const record = props.record;
  if (!record) return [];
  return [
    { label: "API key", value: props.apiKeyName },
    { label: $t("recordPage.detailDialog.summary.provider"), value: props.providerName },
    { label: $t("recordPage.detailDialog.summary.model"), value: value(record.model_name || record.requested_model_name), mono: true },
    { label: "Real model", value: value(record.real_model_name), mono: true },
    { label: "Client API", value: value(record.user_api_type), mono: true },
    { label: "Upstream API", value: value(record.llm_api_type), mono: true },
    { label: "Client IP", value: value(record.client_ip), mono: true },
    { label: "Stream", value: record.is_stream ? $t("common.yes") : $t("common.no") },
    { label: "First byte", value: formatDuration(record.upstream_request_sent_at, record.response_started_to_client_at), mono: true },
    { label: "Total latency", value: formatDuration(record.upstream_request_sent_at, record.completed_at), mono: true },
    { label: "Tokens", value: value(record.total_tokens), mono: true },
    { label: "Cost", value: formatPrice(record.estimated_cost_nanos, record.estimated_cost_currency), mono: true },
    { label: "Received", value: formatDate(record.request_received_at) },
    { label: "Completed", value: record.completed_at ? formatDate(record.completed_at) : emptyValue },
  ];
});
</script>
