<template>
  <section class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader
      :title="t('modelEditPage.requestPatch.title')"
      :help="t('modelEditPage.requestPatch.description')"
      :help-label="t('modelEditPage.requestPatch.title')"
    >
      <template #actions>
      <Button
        variant="ghost"
        size="sm"
        class="w-full sm:w-auto"
        :disabled="isLoading || isRefreshing"
        @click="handleRefresh"
      >
        <RefreshCw
          class="mr-1.5 h-4 w-4"
          :class="{ 'animate-spin': isRefreshing }"
        />
        {{ t("modelEditPage.requestPatch.refresh") }}
      </Button>
      </template>
    </SectionHeader>

    <div v-if="isLoading" class="flex items-center justify-center py-16">
      <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
      <span class="text-sm text-gray-500">
        {{ t("modelEditPage.requestPatch.loading") }}
      </span>
    </div>

    <div
      v-else-if="loadError"
      class="mt-4 rounded-lg border border-red-200 bg-red-50 px-4 py-5"
    >
      <p class="text-sm font-medium text-red-700">
        {{ loadError }}
      </p>
      <Button class="mt-3" variant="outline" size="sm" @click="handleRefresh">
        {{ t("common.retry") }}
      </Button>
    </div>

    <div v-else class="mt-5 space-y-6">
      <div
        v-if="hasConflicts"
        class="rounded-lg border border-red-200 bg-red-50 px-4 py-4"
      >
        <div class="flex items-start gap-3">
          <ShieldAlert class="mt-0.5 h-5 w-5 shrink-0 text-red-600" />
          <div class="min-w-0 flex-1">
            <p class="text-sm font-semibold text-red-800">
              {{ t("modelEditPage.requestPatch.conflictBannerTitle") }}
            </p>
            <p class="mt-1 text-sm text-red-700">
              {{ t("modelEditPage.requestPatch.conflictBannerDescription") }}
            </p>
            <div
              class="mt-4 overflow-hidden rounded-lg border border-red-100 bg-white/80"
            >
              <div
                v-for="conflict in conflicts"
                :key="`${conflict.provider_rule_id}-${conflict.model_rule_id}-${conflict.provider_target}-${conflict.model_target}`"
                class="border-t border-red-100 px-4 py-3 first:border-t-0"
              >
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="destructive" class="font-mono text-[11px]">
                    {{ conflict.placement }}
                  </Badge>
                  <Badge variant="outline" class="font-mono text-[11px]">
                    #{{ conflict.provider_rule_id }} -> #{{ conflict.model_rule_id }}
                  </Badge>
                </div>
                <div class="mt-2 grid gap-2 text-sm text-red-800 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-red-600">
                      {{ t("modelEditPage.requestPatch.conflictFields.providerTarget") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-xs text-red-900">
                      {{ conflict.provider_target }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-red-600">
                      {{ t("modelEditPage.requestPatch.conflictFields.modelTarget") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-xs text-red-900">
                      {{ conflict.model_target }}
                    </p>
                  </div>
                </div>
                <p class="mt-2 text-sm text-red-700">
                  {{ conflict.reason }}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      <RequestPatchRulesPanel
        owner-kind="model"
        :owner-ready="!!modelId"
        :rules="directRules"
        :actions="actions"
        :get-rule-state="getDirectRuleState"
        :get-rule-trace="getDirectRuleTrace"
        @changed="refreshExplainState(false)"
      />

      <div class="border-t border-gray-100 pt-5">
        <div class="space-y-3">
          <SectionHeader
            :title="t('modelEditPage.requestPatch.inheritedTitle')"
            :help="t('modelEditPage.requestPatch.inheritedDescription', { provider: providerLabel })"
            :help-label="t('modelEditPage.requestPatch.inheritedTitle')"
          />

          <div
            v-if="inheritedRules.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.requestPatch.emptyInherited") }}
          </div>

          <div
            v-else
            class="divide-y divide-gray-100 border-y border-gray-100"
          >
            <div
              v-for="item in inheritedRules"
              :key="item.rule.id"
              class="py-4"
            >
              <div class="space-y-3">
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ item.rule.placement }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-[11px]">
                    {{ item.rule.operation }}
                  </Badge>
                  <Badge
                    :variant="getInheritedRuleState(item).variant"
                    class="text-[11px]"
                  >
                    {{ getInheritedRuleState(item).label }}
                  </Badge>
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ t("modelEditPage.requestPatch.origin.ProviderDirect") }}
                  </Badge>
                </div>

                <p class="break-all font-mono text-sm text-gray-900">
                  {{ item.rule.target }}
                </p>

                <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.value") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-sm text-gray-700">
                      {{ formatRequestPatchValueForDisplay(item.rule.value_json) }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.description") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ item.rule.description || t("modelEditPage.requestPatch.noDescription") }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.source") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ providerLabel }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.trace") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ getInheritedRuleTrace(item) }}
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="border-t border-gray-100 pt-5">
        <div class="space-y-3">
          <SectionHeader
            :title="t('modelEditPage.requestPatch.effectiveTitle')"
            :help="t('modelEditPage.requestPatch.effectiveDescription')"
            :help-label="t('modelEditPage.requestPatch.effectiveTitle')"
          />

          <div
            v-if="effectiveRules.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.requestPatch.emptyEffective") }}
          </div>

          <div
            v-else
            class="divide-y divide-gray-100 border-y border-gray-100"
          >
            <div
              v-for="rule in effectiveRules"
              :key="`${rule.source_rule_id}-${rule.target}`"
              class="py-4"
            >
              <div class="space-y-3">
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ rule.placement }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-[11px]">
                    {{ rule.operation }}
                  </Badge>
                  <Badge class="text-[11px]" :variant="rule.source_origin === 'ModelDirect' ? 'default' : 'secondary'">
                    {{ t(`modelEditPage.requestPatch.origin.${rule.source_origin}`) }}
                  </Badge>
                </div>

                <p class="break-all font-mono text-sm text-gray-900">
                  {{ rule.target }}
                </p>

                <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.value") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-sm text-gray-700">
                      {{ formatRequestPatchValueForDisplay(rule.value_json) }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.description") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ rule.description || t("modelEditPage.requestPatch.noDescription") }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.sourceRule") }}
                    </p>
                    <p class="mt-1 font-mono text-xs text-gray-600">
                      #{{ rule.source_rule_id }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.trace") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ getEffectiveRuleTrace(rule) }}
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="border-t border-gray-100 pt-5">
        <div class="space-y-3">
          <SectionHeader
            :title="t('modelEditPage.requestPatch.explainTitle')"
            :help="t('modelEditPage.requestPatch.explainDescription')"
            :help-label="t('modelEditPage.requestPatch.explainTitle')"
          />

          <div
            v-if="explainEntries.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.requestPatch.emptyExplain") }}
          </div>

          <div
            v-else
            class="divide-y divide-gray-100 border-y border-gray-100"
          >
            <div
              v-for="entry in explainEntries"
              :key="entry.rule.id"
              class="py-4"
            >
              <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0 flex-1 space-y-3">
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="font-mono text-[11px]">
                      {{ entry.rule.placement }}
                    </Badge>
                    <Badge
                      class="text-[11px]"
                      :variant="entry.origin === 'ModelDirect' ? 'default' : 'secondary'"
                    >
                      {{ t(`modelEditPage.requestPatch.origin.${entry.origin}`) }}
                    </Badge>
                    <Badge
                      :variant="getExplainStatus(entry.status).variant"
                      class="text-[11px]"
                    >
                      {{ getExplainStatus(entry.status).label }}
                    </Badge>
                  </div>

                  <p class="break-all font-mono text-sm text-gray-900">
                    {{ entry.rule.target }}
                  </p>

                  <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                    <div>
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.requestPatch.fields.ruleId") }}
                      </p>
                      <p class="mt-1 font-mono text-xs text-gray-600">#{{ entry.rule.id }}</p>
                    </div>
                    <div>
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.requestPatch.fields.trace") }}
                      </p>
                      <p class="mt-1 text-sm text-gray-600">
                        {{ entry.message || t("modelEditPage.requestPatch.messages.noRuntimeTrace") }}
                      </p>
                    </div>
                  </div>
                </div>

                <Popover
                  v-if="entry.message || entry.effective_rule_id !== null || entry.conflict_with_rule_ids.length > 0"
                >
                  <PopoverTrigger as-child>
                    <Button variant="ghost" size="sm" class="w-full sm:w-auto">
                      {{ t("modelEditPage.requestPatch.details") }}
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent
                    align="end"
                    class="w-80 border-gray-200 bg-white p-3 text-sm text-gray-700"
                  >
                    <div class="space-y-3">
                      <div v-if="entry.message" class="rounded-md bg-gray-50 px-3 py-2">
                        {{ entry.message }}
                      </div>
                      <div v-if="entry.effective_rule_id !== null">
                        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                          {{ t("modelEditPage.requestPatch.fields.effectiveRule") }}
                        </p>
                        <p class="mt-1 font-mono text-xs text-gray-700">
                          #{{ entry.effective_rule_id }}
                        </p>
                      </div>
                      <div v-if="entry.conflict_with_rule_ids.length > 0">
                        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                          {{ t("modelEditPage.requestPatch.fields.conflictsWith") }}
                        </p>
                        <p class="mt-1 font-mono text-xs text-gray-700">
                          {{ formatRuleIds(entry.conflict_with_rule_ids) }}
                        </p>
                      </div>
                    </div>
                  </PopoverContent>
                </Popover>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { formatRequestPatchValueForDisplay } from "@/utils/requestPatch";
import SectionHeader from "@/components/SectionHeader.vue";
import RequestPatchRulesPanel from "@/components/request-patch/RequestPatchRulesPanel.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Loader2,
  RefreshCw,
  ShieldAlert,
} from "lucide-vue-next";
import { useModelRequestPatch } from "../composables/useModelRequestPatch";

const props = defineProps<{
  modelId: number | null;
  providerName?: string | null;
  providerKey?: string | null;
}>();

const { t } = useI18n();

const modelIdRef = computed(() => props.modelId);
const providerLabel = computed(() => {
  const name = props.providerName?.trim();
  if (name) return name;

  const key = props.providerKey?.trim();
  if (key) return key;

  return t("modelEditPage.requestPatch.providerFallback");
});

const {
  isLoading,
  isRefreshing,
  loadError,
  directRules,
  inheritedRules,
  effectiveRules,
  explainEntries,
  conflicts,
  hasConflicts,
  actions,
  formatRuleIds,
  getExplainStatus,
  getDirectRuleState,
  getDirectRuleTrace,
  getInheritedRuleState,
  getInheritedRuleTrace,
  getEffectiveRuleTrace,
  refreshExplainState,
  handleRefresh,
} = useModelRequestPatch(modelIdRef, providerLabel);
</script>
