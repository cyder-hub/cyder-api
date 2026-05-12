<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import StatsStrip from "@/components/StatsStrip.vue";
import type { DashboardKpiCardItem } from "../types";

const props = defineProps<{
  loading: boolean;
  error: string | null;
  cards: DashboardKpiCardItem[];
}>();

const { t: $t } = useI18n();

const statsItems = computed(() =>
  props.cards.map((card) => ({
    key: card.key,
    label: card.label,
    value: card.value,
    secondary: card.description,
  })),
);
</script>

<template>
  <StatsStrip
    :items="statsItems"
    :loading="props.loading"
    :error="props.error ? $t('dashboard.errorLoading', { error: props.error }) : null"
    :loading-text="$t('common.loading')"
  />
</template>
