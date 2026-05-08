<script setup lang="ts">
import { BellRing, CheckCircle2, RefreshCcw, ShieldOff, XCircle } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { AlertEvent, NotificationDelivery } from "@/services/types";
import type { AlertActionKey } from "../types";
import {
  alertDeliveryBadgeClass,
  formatAlertDateTime,
} from "../composables/alertViewModel";

defineProps<{
  selectedAlert: AlertEvent | null;
  deliveries: NotificationDelivery[];
  isDetailLoading: boolean;
  deliveryError: string | null;
  actionLoading: AlertActionKey | null;
  canAcknowledge: boolean;
  canSuppress: boolean;
  canUnsuppress: boolean;
  canResolve: boolean;
}>();

defineEmits<{
  refreshAlert: [];
  refreshDeliveries: [alertId: number];
  acknowledge: [];
  suppress: [];
  unsuppress: [];
  resolve: [];
}>();

const ackNote = defineModel<string>("ackNote", { required: true });
const suppressUntil = defineModel<string>("suppressUntil", { required: true });
const suppressReason = defineModel<string>("suppressReason", { required: true });
</script>

<template>
  <aside class="rounded-lg border border-gray-200 bg-white">
    <div v-if="!selectedAlert" class="flex flex-col items-center justify-center px-6 py-20 text-gray-500">
      <BellRing class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
      <p class="text-sm font-medium">{{ $t("alertsPage.detail.empty") }}</p>
    </div>
    <div v-else class="divide-y divide-gray-100">
      <div class="px-4 py-4">
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <p class="text-base font-semibold text-gray-900">{{ selectedAlert.title }}</p>
            <p class="mt-1 break-words text-sm text-gray-500">{{ selectedAlert.summary }}</p>
          </div>
          <Button
            variant="ghost"
            size="sm"
            :disabled="isDetailLoading"
            :aria-label="$t('common.refresh')"
            :title="$t('common.refresh')"
            @click="$emit('refreshAlert')"
          >
            <RefreshCcw class="h-4 w-4" :class="{ 'animate-spin': isDetailLoading }" />
          </Button>
        </div>
        <dl class="mt-4 grid grid-cols-1 gap-3 text-xs">
          <div>
            <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.fingerprint") }}</dt>
            <dd class="mt-1 break-all font-mono text-gray-700">{{ selectedAlert.fingerprint }}</dd>
          </div>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.scope") }}</dt>
              <dd class="mt-1 font-mono text-gray-700">{{ selectedAlert.scope_type }}:{{ selectedAlert.scope_id }}</dd>
            </div>
            <div>
              <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.count") }}</dt>
              <dd class="mt-1 font-mono text-gray-700">{{ selectedAlert.occurrence_count }}</dd>
            </div>
          </div>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.firstSeen") }}</dt>
              <dd class="mt-1 text-gray-700">{{ formatAlertDateTime(selectedAlert.first_seen_at) }}</dd>
            </div>
            <div>
              <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.lastSeen") }}</dt>
              <dd class="mt-1 text-gray-700">{{ formatAlertDateTime(selectedAlert.last_seen_at) }}</dd>
            </div>
          </div>
        </dl>
      </div>

      <div class="space-y-3 px-4 py-4">
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("alertsPage.action.ackNote") }}
          </label>
          <textarea
            v-model="ackNote"
            rows="3"
            class="min-h-20 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-400"
            :placeholder="$t('alertsPage.action.ackNotePlaceholder')"
          />
          <Button
            class="mt-2 w-full"
            :disabled="!canAcknowledge || actionLoading === 'ack'"
            @click="$emit('acknowledge')"
          >
            <CheckCircle2 class="mr-1.5 h-4 w-4" />
            {{ $t("alertsPage.action.acknowledge") }}
          </Button>
        </div>

        <div class="space-y-2 rounded-lg border border-gray-200 p-3">
          <label class="block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("alertsPage.action.suppressUntil") }}
          </label>
          <Input v-model="suppressUntil" type="datetime-local" class="w-full" />
          <Input
            v-model="suppressReason"
            class="w-full"
            :placeholder="$t('alertsPage.action.suppressReasonPlaceholder')"
          />
          <div class="flex flex-col gap-2 sm:flex-row">
            <Button
              variant="outline"
              class="w-full"
              :disabled="!canSuppress || actionLoading === 'suppress'"
              @click="$emit('suppress')"
            >
              <ShieldOff class="mr-1.5 h-4 w-4" />
              {{ $t("alertsPage.action.suppress") }}
            </Button>
            <Button
              variant="ghost"
              class="w-full text-gray-600"
              :disabled="!canUnsuppress || actionLoading === 'unsuppress'"
              @click="$emit('unsuppress')"
            >
              {{ $t("alertsPage.action.unsuppress") }}
            </Button>
          </div>
        </div>

        <Button
          variant="outline"
          class="w-full"
          :disabled="!canResolve || actionLoading === 'resolve'"
          @click="$emit('resolve')"
        >
          <XCircle class="mr-1.5 h-4 w-4" />
          {{ $t("alertsPage.action.resolve") }}
        </Button>
      </div>

      <div class="px-4 py-4">
        <div class="mb-3 flex items-center justify-between gap-2">
          <h2 class="text-sm font-semibold text-gray-900">{{ $t("alertsPage.delivery.title") }}</h2>
          <Button
            variant="ghost"
            size="sm"
            :aria-label="$t('common.refresh')"
            :title="$t('common.refresh')"
            @click="$emit('refreshDeliveries', selectedAlert.id)"
          >
            <RefreshCcw class="h-4 w-4" />
          </Button>
        </div>
        <p v-if="deliveryError" class="mb-3 break-words text-xs text-red-600">{{ deliveryError }}</p>
        <div v-if="!deliveries.length" class="rounded-lg border border-gray-100 px-3 py-6 text-center text-sm text-gray-500">
          {{ $t("alertsPage.delivery.empty") }}
        </div>
        <div v-else class="space-y-2">
          <div
            v-for="delivery in deliveries"
            :key="delivery.id"
            class="rounded-lg border border-gray-100 px-3 py-2.5"
          >
            <div class="flex items-start justify-between gap-2">
              <div class="min-w-0">
                <p class="font-mono text-xs text-gray-700">{{ delivery.event_type }}</p>
                <p class="mt-1 text-xs text-gray-500">
                  {{ $t("alertsPage.delivery.attempts", { count: delivery.attempt_count }) }}
                </p>
              </div>
              <Badge :class="alertDeliveryBadgeClass(delivery.status)" class="font-mono text-[11px]">
                {{ $t(`alertsPage.delivery.status.${delivery.status}`) }}
              </Badge>
            </div>
            <p class="mt-2 text-xs text-gray-500">
              {{ $t("alertsPage.delivery.nextAttempt") }} {{ formatAlertDateTime(delivery.next_attempt_at) }}
            </p>
            <p v-if="delivery.last_error" class="mt-1 break-words text-xs text-red-600">
              {{ delivery.last_error }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </aside>
</template>
