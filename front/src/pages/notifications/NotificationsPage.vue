<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { AlertCircle, Loader2, Plus, RefreshCcw } from "lucide-vue-next";

import PageHeader from "@/components/PageHeader.vue";
import StatsStrip from "@/components/StatsStrip.vue";
import { Button } from "@/components/ui/button";
import NotificationChannelDialog from "./components/NotificationChannelDialog.vue";
import NotificationChannelTable from "./components/NotificationChannelTable.vue";
import NotificationDeliveryTable from "./components/NotificationDeliveryTable.vue";
import { useNotificationChannels } from "./composables/useNotificationChannels";
import { useNotificationDeliveries } from "./composables/useNotificationDeliveries";
import { buildNotificationSummaryCounts } from "./composables/notificationViewModel";
import type { NotificationSummaryCard } from "./types";

const { t } = useI18n();

const deliveryState = useNotificationDeliveries();
const channelState = useNotificationChannels({
  afterMutation: deliveryState.loadDeliveries,
});

const summaryCards = computed<NotificationSummaryCard[]>(() => {
  const counts = buildNotificationSummaryCounts(
    channelState.channels.value,
    deliveryState.deliveries.value,
  );
  return [
    {
      key: "channels",
      label: t("notificationPage.summary.channels"),
      value: counts.channels,
    },
    {
      key: "enabled",
      label: t("notificationPage.summary.enabled"),
      value: counts.enabled,
    },
    {
      key: "failed",
      label: t("notificationPage.summary.failed"),
      value: counts.failed,
    },
    {
      key: "retrying",
      label: t("notificationPage.summary.retrying"),
      value: counts.retrying,
    },
  ];
});

const loadPage = async () => {
  try {
    await channelState.loadChannels();
    await deliveryState.loadDeliveries();
  } catch {
    // The channel composable owns the operator-facing error state.
  }
};

onMounted(() => {
  void loadPage();
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <PageHeader :title="$t('notificationPage.title')">
        <template #actions>
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="channelState.isRefreshing.value"
            @click="loadPage"
          >
            <RefreshCcw
              class="mr-1.5 h-4 w-4"
              :class="{ 'animate-spin': channelState.isRefreshing.value }"
            />
            {{ $t("common.refresh") }}
          </Button>
          <Button class="w-full sm:w-auto" @click="channelState.openCreateDialog">
            <Plus class="mr-1.5 h-4 w-4" />
            {{ $t("notificationPage.actions.newChannel") }}
          </Button>
        </template>
      </PageHeader>

      <StatsStrip :items="summaryCards" grid-class="grid-cols-2 sm:grid-cols-4" />

      <div v-if="channelState.isLoading.value" class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("notificationPage.loading") }}</span>
      </div>
      <div v-else-if="channelState.error.value" class="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-white py-20">
        <AlertCircle class="mb-4 h-10 w-10 stroke-1 text-red-400" />
        <span class="max-w-2xl break-words text-center text-sm font-medium text-red-500">
          {{ channelState.error.value }}
        </span>
      </div>
      <template v-else>
        <NotificationChannelTable
          :channels="channelState.channels.value"
          :test-loading-id="channelState.testLoadingId.value"
          @test="channelState.testChannel"
          @edit="channelState.openEditDialog"
          @delete="channelState.deleteChannel"
        />

        <NotificationDeliveryTable
          v-model:status="deliveryState.deliveryStatus.value"
          v-model:channel="deliveryState.deliveryChannel.value"
          :channels="channelState.channels.value"
          :deliveries="deliveryState.deliveries.value"
          :loading="deliveryState.deliveryLoading.value"
          :error="deliveryState.deliveryError.value"
          :status-options="deliveryState.statusOptions.value"
          @refresh="deliveryState.loadDeliveries"
        />
      </template>

      <NotificationChannelDialog
        v-model:open="channelState.isChannelDialogOpen.value"
        v-model:draft="channelState.draft.value"
        :editing-channel="channelState.editingChannel.value"
        :save-loading="channelState.saveLoading.value"
        @save="channelState.saveChannel"
      />
    </div>
  </div>
</template>
