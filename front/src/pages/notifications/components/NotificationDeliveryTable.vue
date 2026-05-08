<script setup lang="ts">
import { BellRing, Loader2, RefreshCcw } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  NotificationChannel,
  NotificationDelivery,
} from "@/services/types";
import type { DeliveryStatusFilter, NotificationSelectOption } from "../types";
import {
  formatNotificationDateTime,
  notificationDeliveryBadgeClass,
} from "../composables/notificationViewModel";

defineProps<{
  channels: NotificationChannel[];
  deliveries: NotificationDelivery[];
  loading: boolean;
  error: string | null;
  statusOptions: NotificationSelectOption<DeliveryStatusFilter>[];
}>();

defineEmits<{
  refresh: [];
}>();

const deliveryStatus = defineModel<DeliveryStatusFilter>("status", {
  required: true,
});
const deliveryChannel = defineModel<string>("channel", { required: true });
</script>

<template>
  <section class="rounded-lg border border-gray-200 bg-white">
    <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 lg:flex-row lg:items-end lg:justify-between">
      <div class="min-w-0">
        <h2 class="text-base font-semibold text-gray-900">
          {{ $t("notificationPage.delivery.title") }}
        </h2>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("notificationPage.delivery.description") }}
        </p>
      </div>
      <div class="grid grid-cols-1 gap-2 sm:grid-cols-[10rem_12rem_auto]">
        <Select v-model="deliveryStatus" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="option in statusOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Select v-model="deliveryChannel" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="$t('notificationPage.delivery.allChannels')" />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem value="all">{{ $t("notificationPage.delivery.allChannels") }}</SelectItem>
            <SelectItem
              v-for="channel in channels"
              :key="channel.id"
              :value="String(channel.id)"
            >
              {{ channel.name }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Button variant="outline" :disabled="loading" @click="$emit('refresh')">
          <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': loading }" />
          {{ $t("common.refresh") }}
        </Button>
      </div>
    </div>

    <p v-if="error" class="border-b border-red-100 bg-red-50 px-4 py-3 text-sm text-red-600">
      {{ error }}
    </p>
    <div v-if="loading" class="flex items-center justify-center py-16 text-gray-500">
      <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
      <span class="text-sm">{{ $t("notificationPage.delivery.loading") }}</span>
    </div>
    <div v-else-if="!deliveries.length" class="flex flex-col items-center justify-center py-20 text-gray-500">
      <BellRing class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
      <p class="text-sm font-medium">{{ $t("notificationPage.delivery.empty") }}</p>
    </div>
    <div v-else class="app-scroll-x">
      <table class="min-w-full divide-y divide-gray-100 text-sm">
        <thead class="bg-gray-50/80">
          <tr>
            <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("notificationPage.delivery.table.alert") }}
            </th>
            <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("notificationPage.delivery.table.status") }}
            </th>
            <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("notificationPage.delivery.table.attempt") }}
            </th>
            <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("notificationPage.delivery.table.nextAttempt") }}
            </th>
            <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("notificationPage.delivery.table.deliveredAt") }}
            </th>
            <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("notificationPage.delivery.table.error") }}
            </th>
          </tr>
        </thead>
        <tbody class="divide-y divide-gray-100 bg-white">
          <tr v-for="delivery in deliveries" :key="delivery.id">
            <td class="px-4 py-3 align-top">
              <p class="max-w-[18rem] truncate font-mono text-xs text-gray-700">
                {{ delivery.alert_fingerprint }}
              </p>
              <p class="mt-1 font-mono text-xs text-gray-400">{{ delivery.event_type }}</p>
            </td>
            <td class="px-4 py-3 align-top">
              <Badge :class="notificationDeliveryBadgeClass(delivery.status)" class="font-mono text-[11px]">
                {{ $t(`notificationPage.delivery.status.${delivery.status}`) }}
              </Badge>
            </td>
            <td class="px-4 py-3 align-top font-mono text-xs text-gray-700">
              {{ delivery.attempt_count }}
              <span v-if="delivery.last_status_code" class="text-gray-400">
                / {{ delivery.last_status_code }}
              </span>
            </td>
            <td class="px-4 py-3 align-top text-xs text-gray-600">
              {{ formatNotificationDateTime(delivery.next_attempt_at) }}
            </td>
            <td class="px-4 py-3 align-top text-xs text-gray-600">
              {{ formatNotificationDateTime(delivery.delivered_at) }}
            </td>
            <td class="max-w-[22rem] px-4 py-3 align-top">
              <p class="break-words text-xs text-red-600">
                {{ delivery.last_error || "-" }}
              </p>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
