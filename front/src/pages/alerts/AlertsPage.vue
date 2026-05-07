<script setup lang="ts">
import { AlertCircle, Loader2, RefreshCcw } from "lucide-vue-next";

import { Button } from "@/components/ui/button";
import AlertDetailDrawer from "./components/AlertDetailDrawer.vue";
import AlertFilters from "./components/AlertFilters.vue";
import AlertTable from "./components/AlertTable.vue";
import { useAlertActions } from "./composables/useAlertActions";
import { useAlertList } from "./composables/useAlertList";

const {
  deliveries,
  selectedAlert,
  isLoading,
  isRefreshing,
  isDetailLoading,
  error,
  deliveryError,
  filters,
  statusOptions,
  severityOptions,
  scopeOptions,
  booleanOptions,
  visibleAlerts,
  summaryCards,
  offset,
  canGoPrevious,
  canGoNext,
  loadAlerts,
  loadDeliveries,
  selectAlert,
  refreshSelectedAlert,
  goToPreviousPage,
  goToNextPage,
} = useAlertList();

const {
  actionLoading,
  ackNote,
  suppressUntil,
  suppressReason,
  canAcknowledge,
  canSuppress,
  canUnsuppress,
  canResolve,
  acknowledgeSelected,
  suppressSelected,
  unsuppressSelected,
  resolveSelected,
} = useAlertActions({
  selectedAlert,
  reloadAlerts: loadAlerts,
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
            {{ $t("alertsPage.title") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("alertsPage.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="isRefreshing"
            @click="loadAlerts"
          >
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
            {{ $t("common.refresh") }}
          </Button>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-gray-200 bg-gray-100 sm:grid-cols-4">
        <div v-for="card in summaryCards" :key="card.key" class="bg-white px-4 py-3">
          <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
            {{ card.label }}
          </p>
          <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
            {{ card.value }}
          </p>
        </div>
      </div>

      <AlertFilters
        v-model:filters="filters"
        :status-options="statusOptions"
        :severity-options="severityOptions"
        :scope-options="scopeOptions"
        :boolean-options="booleanOptions"
        @refresh="loadAlerts({ resetOffset: true })"
      />

      <div v-if="isLoading" class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("alertsPage.loading") }}</span>
      </div>
      <div v-else-if="error" class="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-white py-20">
        <AlertCircle class="mb-4 h-10 w-10 stroke-1 text-red-400" />
        <span class="max-w-2xl break-words text-center text-sm font-medium text-red-500">
          {{ error }}
        </span>
      </div>
      <div v-else class="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_26rem]">
        <div class="space-y-3">
          <AlertTable
            :alerts="visibleAlerts"
            :selected-alert-id="selectedAlert?.id ?? null"
            @select="selectAlert"
          />
          <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <p class="text-xs text-gray-500">
              {{ $t("pagination.items") }}
              {{ visibleAlerts.length ? offset + 1 : offset }} - {{ offset + visibleAlerts.length }}
            </p>
            <div class="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                :disabled="!canGoPrevious || isRefreshing"
                @click="goToPreviousPage"
              >
                {{ $t("pagination.previousPage") }}
              </Button>
              <Button
                variant="outline"
                size="sm"
                :disabled="!canGoNext || isRefreshing"
                @click="goToNextPage"
              >
                {{ $t("pagination.nextPage") }}
              </Button>
            </div>
          </div>
        </div>

        <AlertDetailDrawer
          v-model:ack-note="ackNote"
          v-model:suppress-until="suppressUntil"
          v-model:suppress-reason="suppressReason"
          :selected-alert="selectedAlert"
          :deliveries="deliveries"
          :is-detail-loading="isDetailLoading"
          :delivery-error="deliveryError"
          :action-loading="actionLoading"
          :can-acknowledge="canAcknowledge"
          :can-suppress="canSuppress"
          :can-unsuppress="canUnsuppress"
          :can-resolve="canResolve"
          @refresh-alert="refreshSelectedAlert"
          @refresh-deliveries="loadDeliveries"
          @acknowledge="acknowledgeSelected"
          @suppress="suppressSelected"
          @unsuppress="unsuppressSelected"
          @resolve="resolveSelected"
        />
      </div>
    </div>
  </div>
</template>
