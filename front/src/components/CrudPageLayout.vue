<script setup lang="ts">
import PageHeader from "@/components/PageHeader.vue";

withDefaults(
  defineProps<{
    title: string;
    description?: string;
    help?: string;
    helpLabel?: string;
    loading?: boolean;
    error?: string | null;
    empty?: boolean;
    headerClass?: string;
    pageClass?: string;
    shellClass?: string;
    contentClass?: string;
  }>(),
  {
    description: "",
    help: "",
    helpLabel: "Page help",
    loading: false,
    error: null,
    empty: false,
    headerClass:
      "flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between",
    pageClass: "",
    shellClass: "",
    contentClass: "",
  },
);
</script>

<template>
  <div class="app-page" :class="pageClass">
    <div class="app-page-shell" :class="shellClass">
      <PageHeader
        :title="title"
        :help="help"
        :help-label="helpLabel"
        :class="headerClass"
      >
        <template v-if="$slots.help" #help>
          <slot name="help" />
        </template>
        <template #actions>
          <slot name="actions" />
        </template>
      </PageHeader>

      <div class="app-section" :class="contentClass">
        <template v-if="loading">
          <slot name="loading" />
        </template>
        <template v-else-if="error">
          <slot name="error" :error="error" />
        </template>
        <template v-else-if="empty">
          <slot name="empty" />
        </template>
        <template v-else>
          <slot />
        </template>
      </div>

      <slot name="modals" />
    </div>
  </div>
</template>
