<script setup lang="ts">
import { Copy, KeyRound, TriangleAlert } from "lucide-vue-next";
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
import type { ApiKeyReveal } from "@/services/types";

defineProps<{
  secret: ApiKeyReveal | null;
}>();

defineEmits<{
  (event: "copy", secret: string): void;
  (event: "acknowledge"): void;
}>();

const { t } = useI18n();
</script>

<template>
  <Dialog :open="secret !== null" @update:open="() => undefined">
    <DialogContent
      :show-close-button="false"
      class="sm:max-w-xl"
      @escape-key-down.prevent
      @pointer-down-outside.prevent
      @interact-outside.prevent
    >
      <DialogHeader>
        <div class="mb-1 flex h-10 w-10 items-center justify-center rounded-lg bg-gray-100">
          <KeyRound class="h-5 w-5 text-gray-700" />
        </div>
        <DialogTitle>
          {{
            t(
              secret?.can_reveal
                ? "apiKeyPage.issuedSecret.recoverableTitle"
                : "apiKeyPage.issuedSecret.title",
            )
          }}
        </DialogTitle>
        <DialogDescription class="text-left leading-6">
          {{
            t(
              secret?.can_reveal
                ? "apiKeyPage.issuedSecret.recoverableDescription"
                : "apiKeyPage.issuedSecret.description",
            )
          }}
        </DialogDescription>
      </DialogHeader>

      <div v-if="secret" class="space-y-3">
        <div class="rounded-lg border border-gray-200 bg-gray-50 p-3 sm:p-4">
          <p class="text-xs font-medium text-gray-500">
            {{ secret.name }} · {{ secret.key_prefix }}...{{ secret.key_last4 }}
          </p>
          <textarea
            readonly
            rows="3"
            class="mt-2 flex w-full resize-none rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none"
            :value="secret.api_key"
          />
          <Button
            variant="outline"
            class="mt-3 w-full sm:w-auto"
            @click="$emit('copy', secret.api_key)"
          >
            <Copy class="mr-1.5 h-4 w-4" />
            {{ t("apiKeyPage.actions.copySecret") }}
          </Button>
        </div>

        <div
          v-if="!secret.can_reveal"
          class="flex gap-2 rounded-lg border border-gray-200 bg-white px-3 py-3 text-xs leading-5 text-gray-600"
        >
          <TriangleAlert class="mt-0.5 h-4 w-4 shrink-0 text-gray-500" />
          <p>{{ t("apiKeyPage.issuedSecret.responseLoss") }}</p>
        </div>
      </div>

      <DialogFooter>
        <Button class="w-full sm:w-auto" @click="$emit('acknowledge')">
          {{
            t(
              secret?.can_reveal
                ? "apiKeyPage.issuedSecret.close"
                : "apiKeyPage.issuedSecret.acknowledge",
            )
          }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
