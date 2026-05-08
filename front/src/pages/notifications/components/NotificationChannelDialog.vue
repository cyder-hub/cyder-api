<script setup lang="ts">
import { Loader2 } from "lucide-vue-next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { NotificationChannel } from "@/services/types";
import type { ChannelDraft } from "../types";

defineProps<{
  editingChannel: NotificationChannel | null;
  saveLoading: boolean;
}>();

defineEmits<{
  save: [];
}>();

const open = defineModel<boolean>("open", { required: true });
const draft = defineModel<ChannelDraft>("draft", { required: true });
</script>

<template>
  <Dialog :open="open" @update:open="(value) => (open = value)">
    <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{
            editingChannel
              ? $t("notificationPage.dialog.editTitle")
              : $t("notificationPage.dialog.createTitle")
          }}
        </DialogTitle>
      </DialogHeader>
      <div class="space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("notificationPage.dialog.channelKey") }}
          </label>
          <Input v-model="draft.channel_key" :disabled="!!editingChannel" class="w-full" />
        </div>
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("notificationPage.dialog.name") }}
          </label>
          <Input v-model="draft.name" class="w-full" />
        </div>
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("notificationPage.dialog.endpointUrl") }}
          </label>
          <Input v-model="draft.endpoint_url" class="w-full" />
        </div>
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("notificationPage.dialog.signingSecret") }}
          </label>
          <Input v-model="draft.signing_secret" type="password" class="w-full" />
        </div>
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("notificationPage.dialog.headersJson") }}
          </label>
          <textarea
            v-model="draft.headers_json"
            class="min-h-24 w-full rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-xs text-gray-800 outline-none transition focus:border-gray-400 focus:ring-2 focus:ring-gray-100"
            spellcheck="false"
          />
        </div>
        <div>
          <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("notificationPage.dialog.cooldownSeconds") }}
          </label>
          <Input v-model="draft.cooldown_seconds" type="number" min="0" max="86400" class="w-full" />
        </div>
        <label class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
          <span class="text-sm text-gray-700">{{ $t("notificationPage.dialog.enabled") }}</span>
          <Checkbox
            :model-value="draft.is_enabled"
            @update:model-value="(value) => (draft.is_enabled = value === true)"
          />
        </label>
        <label v-if="editingChannel" class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
          <span class="text-sm text-gray-700">{{ $t("notificationPage.dialog.clearSecret") }}</span>
          <Checkbox
            :model-value="draft.clear_signing_secret"
            @update:model-value="(value) => (draft.clear_signing_secret = value === true)"
          />
        </label>
        <label v-if="editingChannel" class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
          <span class="text-sm text-gray-700">{{ $t("notificationPage.dialog.clearHeaders") }}</span>
          <Checkbox
            :model-value="draft.clear_headers"
            @update:model-value="(value) => (draft.clear_headers = value === true)"
          />
        </label>
      </div>
      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="text-gray-600" @click="open = false">
          {{ $t("common.cancel") }}
        </Button>
        <Button :disabled="saveLoading" @click="$emit('save')">
          <Loader2 v-if="saveLoading" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ $t("common.save") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
