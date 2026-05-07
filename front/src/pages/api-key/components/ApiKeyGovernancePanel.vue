<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { Activity, Gauge, KeyRound, Shield } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { formatTimestamp } from "@/utils/datetime";
import type { ApiKeyDetail, ApiKeyRuntimeSnapshot } from "@/services/types";
import {
  aclRuleTarget,
  actionLabel,
  billedAmountLabel,
  buildApiKeyGovernanceItems,
  buildRuntimeRejectionView,
  lifecycleLabel,
  modelOverrideTargetLabel,
  runtimeRejectionBadgeClass,
  scopeLabel,
} from "../composables/useApiKeyDetail";

const props = defineProps<{
  detail: ApiKeyDetail;
  runtime: ApiKeyRuntimeSnapshot;
  providerNameById: Map<number, string>;
  modelNameById: Map<number, string>;
  routeNameById: Map<number, string>;
}>();

const { t } = useI18n();

const governanceItems = computed(() => buildApiKeyGovernanceItems(props.detail, t));
const runtimeRejection = computed(() =>
  buildRuntimeRejectionView(props.detail, props.runtime, t),
);
</script>

<template>
  <div class="space-y-6">
    <section class="space-y-3">
      <div class="flex items-center gap-2">
        <Shield class="h-4 w-4 text-gray-400" />
        <h3 class="text-base font-semibold text-gray-900">
          {{ t("apiKeyPage.sections.identityTitle") }}
        </h3>
      </div>
      <dl class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.table.status") }}
          </dt>
          <dd class="mt-1 text-sm font-medium text-gray-900">
            {{ lifecycleLabel(detail, t) }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.table.defaultAction") }}
          </dt>
          <dd class="mt-1 text-sm font-medium text-gray-900">
            {{ actionLabel(detail.default_action, t) }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.table.createdAt") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ formatTimestamp(detail.created_at) || t("common.notAvailable") }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.table.updatedAt") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ formatTimestamp(detail.updated_at) || t("common.notAvailable") }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3 sm:col-span-2">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.table.description") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ detail.description || t("common.notAvailable") }}
          </dd>
        </div>
      </dl>
    </section>

    <section class="space-y-3 border-t border-gray-100 pt-6">
      <div class="flex items-center gap-2">
        <Gauge class="h-4 w-4 text-gray-400" />
        <h3 class="text-base font-semibold text-gray-900">
          {{ t("apiKeyPage.sections.governanceTitle") }}
        </h3>
      </div>
      <dl class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div
          v-for="item in governanceItems"
          :key="item.key"
          class="rounded-lg border border-gray-200 px-3 py-3"
        >
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ item.label }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ item.value }}
          </dd>
        </div>
      </dl>
    </section>

    <section class="space-y-3 border-t border-gray-100 pt-6">
      <div class="flex items-center gap-2">
        <Shield class="h-4 w-4 text-gray-400" />
        <h3 class="text-base font-semibold text-gray-900">
          {{ t("apiKeyPage.sections.aclTitle") }}
        </h3>
      </div>

      <div
        v-if="!detail.acl_rules.length"
        class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500"
      >
        {{ t("apiKeyPage.noRules") }}
      </div>

      <div v-else class="space-y-3">
        <div
          v-for="rule in detail.acl_rules"
          :key="rule.id"
          class="rounded-lg border border-gray-200 px-4 py-3"
        >
          <div class="flex flex-wrap items-center gap-2">
            <Badge variant="outline" class="text-[11px]">
              {{ actionLabel(rule.effect, t) }}
            </Badge>
            <Badge variant="secondary" class="text-[11px]">
              {{ scopeLabel(rule.scope, t) }}
            </Badge>
            <Badge
              :class="
                rule.is_enabled
                  ? 'border border-gray-200 bg-white text-gray-600'
                  : 'border border-gray-200 bg-gray-100 text-gray-400'
              "
              class="text-[11px]"
            >
              {{
                rule.is_enabled
                  ? t("apiKeyPage.rule.enabled")
                  : t("apiKeyPage.rule.disabled")
              }}
            </Badge>
          </div>
          <p class="mt-2 text-sm text-gray-900">
            {{ aclRuleTarget(rule, providerNameById, modelNameById, t) }}
          </p>
          <p class="mt-1 text-xs text-gray-500">
            {{ t("apiKeyPage.rule.priority", { value: rule.priority }) }}
          </p>
          <p v-if="rule.description" class="mt-2 text-sm text-gray-500">
            {{ rule.description }}
          </p>
        </div>
      </div>
    </section>

    <section class="space-y-3 border-t border-gray-100 pt-6">
      <div class="flex items-center gap-2">
        <KeyRound class="h-4 w-4 text-gray-400" />
        <h3 class="text-base font-semibold text-gray-900">
          {{ t("apiKeyPage.sections.overrideTitle") }}
        </h3>
      </div>
      <p class="text-sm text-gray-500">
        {{ t("apiKeyPage.sections.overrideDescription") }}
      </p>

      <div
        v-if="!detail.model_overrides.length"
        class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500"
      >
        {{ t("apiKeyPage.noOverrides") }}
      </div>

      <div v-else class="space-y-3">
        <div
          v-for="item in detail.model_overrides"
          :key="item.id"
          class="rounded-lg border border-gray-200 px-4 py-3"
        >
          <div class="flex flex-wrap items-center gap-2">
            <Badge variant="outline" class="text-[11px]">
              {{ item.source_name }}
            </Badge>
            <Badge variant="secondary" class="text-[11px]">
              {{ modelOverrideTargetLabel(item, routeNameById, t) }}
            </Badge>
            <Badge
              :class="
                item.is_enabled
                  ? 'border border-gray-200 bg-white text-gray-600'
                  : 'border border-gray-200 bg-gray-100 text-gray-400'
              "
              class="text-[11px]"
            >
              {{
                item.is_enabled
                  ? t("apiKeyPage.rule.enabled")
                  : t("apiKeyPage.rule.disabled")
              }}
            </Badge>
          </div>
          <p class="mt-2 text-sm text-gray-900">
            {{
              t("apiKeyPage.override.targetRoute", {
                value: modelOverrideTargetLabel(item, routeNameById, t),
              })
            }}
          </p>
          <p v-if="item.description" class="mt-1 text-sm text-gray-500">
            {{ item.description }}
          </p>
        </div>
      </div>
    </section>

    <section class="space-y-3 border-t border-gray-100 pt-6">
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-2">
          <Activity class="h-4 w-4 text-gray-400" />
          <h3 class="text-base font-semibold text-gray-900">
            {{ t("apiKeyPage.sections.runtimeTitle") }}
          </h3>
        </div>
        <Badge :class="runtimeRejectionBadgeClass(runtimeRejection)" class="text-[11px]">
          {{ runtimeRejection.label }}
        </Badge>
      </div>

      <dl class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.currentConcurrency") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ runtime.current_concurrency }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.currentMinuteRequests") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ runtime.current_minute_request_count }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.dailyRequests") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ runtime.daily_request_count }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.dailyTokens") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ runtime.daily_token_count }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.monthlyTokens") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ runtime.monthly_token_count }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.runtimeRejectionReason") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ runtimeRejection.label }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.dailyBudgetUsage") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ billedAmountLabel(runtime.daily_billed_amounts, t) }}
          </dd>
        </div>
        <div class="rounded-lg border border-gray-200 px-3 py-3">
          <dt class="text-xs uppercase tracking-wide text-gray-500">
            {{ t("apiKeyPage.runtime.monthlyBudgetUsage") }}
          </dt>
          <dd class="mt-1 text-sm text-gray-900">
            {{ billedAmountLabel(runtime.monthly_billed_amounts, t) }}
          </dd>
        </div>
      </dl>
    </section>
  </div>
</template>
