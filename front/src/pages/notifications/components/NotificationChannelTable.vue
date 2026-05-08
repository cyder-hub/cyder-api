<script setup lang="ts">
import { Inbox, Send, Trash2, Webhook } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { NotificationChannel } from "@/services/types";
import {
  formatNotificationDateTime,
  notificationChannelStatusClass,
} from "../composables/notificationViewModel";

defineProps<{
  channels: NotificationChannel[];
  testLoadingId: number | null;
}>();

defineEmits<{
  test: [channel: NotificationChannel];
  edit: [channel: NotificationChannel];
  delete: [channel: NotificationChannel];
}>();
</script>

<template>
  <section class="rounded-lg border border-gray-200 bg-white">
    <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
      <div class="min-w-0">
        <h2 class="text-base font-semibold text-gray-900">
          {{ $t("notificationPage.channels.title") }}
        </h2>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("notificationPage.channels.description") }}
        </p>
      </div>
    </div>

    <div v-if="!channels.length" class="flex flex-col items-center justify-center py-20 text-gray-500">
      <Inbox class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
      <p class="text-sm font-medium">{{ $t("notificationPage.channels.empty") }}</p>
    </div>
    <div v-else class="divide-y divide-gray-100">
      <div
        v-for="channel in channels"
        :key="channel.id"
        class="grid grid-cols-1 gap-3 px-4 py-4 lg:grid-cols-[minmax(0,1fr)_auto]"
      >
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <Webhook class="h-4 w-4 text-gray-400" />
            <p class="font-medium text-gray-900">{{ channel.name }}</p>
            <Badge :class="notificationChannelStatusClass(channel)" class="font-mono text-[11px]">
              {{
                channel.is_enabled
                  ? $t("notificationPage.channels.enabled")
                  : $t("notificationPage.channels.disabled")
              }}
            </Badge>
            <Badge class="border-gray-200 bg-white font-mono text-[11px] text-gray-600">
              {{ channel.channel_key }}
            </Badge>
          </div>
          <p class="mt-2 break-all font-mono text-xs text-gray-500">
            {{ channel.endpoint_url }}
          </p>
          <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-gray-500">
            <span>
              {{ $t("notificationPage.channels.secret") }}
              {{
                channel.signing_secret_redacted
                  ? $t("notificationPage.channels.configured")
                  : $t("notificationPage.channels.notConfigured")
              }}
            </span>
            <span>
              {{ $t("notificationPage.channels.headers") }}
              {{
                channel.headers_json
                  ? $t("notificationPage.channels.configured")
                  : $t("notificationPage.channels.notConfigured")
              }}
            </span>
            <span>{{ $t("notificationPage.channels.cooldown") }} {{ channel.cooldown_seconds }}s</span>
            <span>{{ $t("notificationPage.channels.lastTest") }} {{ formatNotificationDateTime(channel.last_test_at) }}</span>
            <span v-if="channel.last_test_error" class="break-words text-red-600">
              {{ channel.last_test_error }}
            </span>
          </div>
        </div>
        <div class="flex flex-col gap-2 sm:flex-row lg:justify-end">
          <Button
            variant="outline"
            size="sm"
            :disabled="testLoadingId === channel.id"
            @click="$emit('test', channel)"
          >
            <Send class="mr-1.5 h-3.5 w-3.5" />
            {{ $t("notificationPage.actions.test") }}
          </Button>
          <Button variant="ghost" size="sm" @click="$emit('edit', channel)">
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="text-gray-400 hover:text-red-600"
            @click="$emit('delete', channel)"
          >
            <Trash2 class="mr-1.5 h-3.5 w-3.5" />
            {{ $t("common.delete") }}
          </Button>
        </div>
      </div>
    </div>
  </section>
</template>
