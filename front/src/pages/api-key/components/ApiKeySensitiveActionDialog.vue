<script setup lang="ts">
import { computed } from "vue";
import { Loader2 } from "lucide-vue-next";
import { useI18n } from "vue-i18n";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export type ApiKeySensitiveAction = "reveal" | "rotate" | "delete";

const props = defineProps<{
  open: boolean;
  action: ApiKeySensitiveAction;
  targetName: string;
  loading: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  confirm: [];
}>();

const { t } = useI18n();

const title = computed(() => {
  if (props.action === "reveal") {
    return t("apiKeyPage.confirmReveal", { name: props.targetName });
  }
  if (props.action === "rotate") {
    return t("apiKeyPage.confirmRotate", { name: props.targetName });
  }
  return t("apiKeyPage.confirmDelete", { name: props.targetName });
});

const actionLabel = computed(() =>
  props.action === "delete"
    ? t("common.delete")
    : t(`apiKeyPage.actions.${props.action}`),
);

const handleOpenChange = (open: boolean) => {
  if (props.loading) return;
  emit("update:open", open);
};

const submit = () => {
  if (props.loading) return;
  emit("confirm");
};
</script>

<template>
  <Dialog :open="open" @update:open="handleOpenChange">
    <DialogContent class="border border-gray-200 bg-white p-0 sm:max-w-lg">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ title }}
        </DialogTitle>
        <DialogDescription class="text-sm leading-6 text-gray-500">
          {{
            action === "reveal"
              ? t("apiKeyPage.confirmRevealDescription")
              : t("apiKeyPage.confirmSensitiveDescription")
          }}
        </DialogDescription>
      </DialogHeader>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button
          variant="ghost"
          class="w-full sm:w-auto"
          :disabled="loading"
          @click="handleOpenChange(false)"
        >
          {{ t("common.cancel") }}
        </Button>
        <Button
          :variant="action === 'delete' ? 'destructive' : 'default'"
          class="w-full sm:w-auto"
          :disabled="loading"
          @click="submit"
        >
          <Loader2 v-if="loading" class="h-4 w-4 animate-spin" />
          {{ actionLabel }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
