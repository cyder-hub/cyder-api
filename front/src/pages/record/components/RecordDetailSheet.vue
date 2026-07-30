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
            <span class="font-mono text-xs text-gray-500">
              {{ $t("recordPage.detailDialog.recordId") }} #{{ record.id }}
            </span>
            <span v-if="record.upstream_http_status" class="font-mono text-xs text-gray-500">
              HTTP {{ record.upstream_http_status }}
            </span>
          </div>

          <div class="rounded-lg border border-gray-200 bg-gray-50 p-3">
            <div class="flex items-center justify-between gap-3">
              <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("recordPage.detailDialog.gatewayRequestId") }}
              </p>
              <Button
                variant="outline"
                size="sm"
                class="h-8 flex-none gap-1.5"
                :aria-label="$t('recordPage.detailDialog.copyRequestId')"
                @click="copyRequestId"
              >
                <Copy class="h-3.5 w-3.5" />
                {{ $t("recordPage.detailDialog.copy") }}
              </Button>
            </div>
            <p class="mt-2 break-all font-mono text-xs leading-5 text-gray-900">
              {{ record.request_id }}
            </p>
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
            <h3 class="mb-3 text-sm font-semibold text-gray-900">
              {{ $t("recordPage.detailDialog.summary.error") }}
            </h3>
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
import { Copy } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Drawer,
  DrawerContent,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import { toastController } from "@/services/uiFeedback";
import type { RecordRequest } from "@/services/types";
import { copyText } from "@/utils/clipboard";
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

const copyRequestId = async () => {
  const requestId = props.record?.request_id;
  if (!requestId || !(await copyText(requestId))) {
    toastController.error($t("recordPage.detailDialog.copyFailed"));
    return;
  }
  toastController.success($t("recordPage.detailDialog.copySuccess"));
};

const overviewItems = computed(() => {
  const record = props.record;
  if (!record) return [];
  return [
    { label: $t("recordPage.detailDialog.summary.apiKey"), value: props.apiKeyName },
    { label: $t("recordPage.detailDialog.summary.provider"), value: props.providerName },
    { label: $t("recordPage.detailDialog.summary.model"), value: value(record.model_name || record.requested_model_name), mono: true },
    { label: $t("recordPage.detailDialog.summary.realModel"), value: value(record.real_model_name), mono: true },
    { label: $t("recordPage.detailDialog.summary.downstreamProtocol"), value: value(record.downstream_protocol), mono: true },
    { label: $t("recordPage.detailDialog.summary.upstreamProtocol"), value: value(record.upstream_protocol), mono: true },
    { label: $t("recordPage.detailDialog.summary.clientIp"), value: value(record.client_ip), mono: true },
    ...(record.client_request_id
      ? [{ label: $t("recordPage.detailDialog.summary.clientRequestId"), value: record.client_request_id, mono: true }]
      : []),
    { label: $t("recordPage.detailDialog.summary.stream"), value: record.is_stream ? $t("common.yes") : $t("common.no") },
    { label: $t("recordPage.detailDialog.summary.firstByte"), value: formatDuration(record.upstream_request_sent_at, record.response_started_to_client_at), mono: true },
    { label: $t("recordPage.detailDialog.summary.totalLatency"), value: formatDuration(record.upstream_request_sent_at, record.completed_at), mono: true },
    { label: $t("recordPage.detailDialog.summary.tokens"), value: value(record.total_tokens), mono: true },
    { label: $t("recordPage.detailDialog.summary.cost"), value: formatPrice(record.estimated_cost_nanos, record.estimated_cost_currency), mono: true },
    { label: $t("recordPage.detailDialog.summary.received"), value: formatDate(record.request_received_at) },
    { label: $t("recordPage.detailDialog.summary.completed"), value: record.completed_at ? formatDate(record.completed_at) : emptyValue },
  ];
});
</script>
