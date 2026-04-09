<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Api } from "@/services/request";
import type { ApiKeyItem } from "@/store/types";

interface IssueTokenModalProps {
  isOpen: boolean;
  apiKey: ApiKeyItem | null;
}

const props = defineProps<IssueTokenModalProps>();
const emit = defineEmits(["update:isOpen"]);

const { t } = useI18n();

const uid = ref("");
const channel = ref("");
const duration = ref("1y");
const generatedToken = ref<string | null>(null);
const error = ref<string | null>(null);

const durationOptions = computed(() => [
  { value: "1d", label: t("issueTokenModal.durations.1d") },
  { value: "7d", label: t("issueTokenModal.durations.7d") },
  { value: "30d", label: t("issueTokenModal.durations.30d") },
  { value: "1y", label: t("issueTokenModal.durations.1y") },
  { value: "3y", label: t("issueTokenModal.durations.3y") },
  { value: "forever", label: t("issueTokenModal.durations.forever") },
]);

const resetState = () => {
  uid.value = "";
  channel.value = "";
  duration.value = "1y";
  generatedToken.value = null;
  error.value = null;
};

const handleOpenChange = (open: boolean) => {
  if (!open) {
    resetState();
  }
  emit("update:isOpen", open);
};

const handleSubmit = async () => {
  if (!props.apiKey) return;
  if (!uid.value) {
    error.value = t("issueTokenModal.uidRequired");
    return;
  }
  error.value = null;

  let payload: {
    uid: string;
    channel?: string;
    duration?: number;
    end_at?: number;
  } = {
    uid: uid.value,
  };

  if (channel.value) {
    payload.channel = channel.value;
  }

  const d = duration.value;
  if (d === "forever") {
    payload.end_at = 253402297199000; // Year 9999
  } else {
    const day_ms = 24 * 60 * 60 * 1000;
    const durationMap: { [key: string]: number } = {
      "1d": 1 * day_ms,
      "7d": 7 * day_ms,
      "30d": 30 * day_ms,
      "1y": 365 * day_ms,
      "3y": 3 * 365 * day_ms,
    };
    payload.duration = durationMap[d];
  }

  try {
    const response = await Api.issueApiKeyToken(props.apiKey.id, payload);
    generatedToken.value = response;
  } catch (err) {
    error.value = (err as Error).message || t("unknownError");
  }
};

watch(
  () => props.isOpen,
  (isOpen) => {
    if (!isOpen) {
      resetState();
    }
  },
);
</script>

<template>
  <Dialog :open="props.isOpen" @update:open="handleOpenChange">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-lg">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle>{{ t("issueTokenModal.title") }}</DialogTitle>
        <DialogDescription>
          {{
            t("issueTokenModal.description", { name: props.apiKey?.name || "" })
          }}
        </DialogDescription>
      </DialogHeader>

      <div v-if="!generatedToken" class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
        <div class="space-y-2">
          <Label for="uid">{{ t("issueTokenModal.uidLabel") }}</Label>
          <Input
            id="uid"
            v-model="uid"
            :placeholder="t('issueTokenModal.uidPlaceholder')"
            required
          />
        </div>
        <div class="space-y-2">
          <Label for="channel">{{ t("issueTokenModal.channelLabel") }}</Label>
          <Input
            id="channel"
            v-model="channel"
            :placeholder="t('issueTokenModal.channelPlaceholder')"
          />
        </div>
        <div class="space-y-2">
          <Label for="duration">{{ t("issueTokenModal.durationLabel") }}</Label>
          <Select v-model="duration">
            <SelectTrigger id="duration">
              <SelectValue
                :placeholder="t('issueTokenModal.durationPlaceholder')"
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="opt in durationOptions"
                :key="opt.value"
                :value="opt.value"
              >
                {{ opt.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <p v-if="error" class="text-sm text-red-600">{{ error }}</p>
      </div>

      <div v-if="generatedToken" class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
        <p>{{ t("issueTokenModal.tokenGenerated") }}</p>
        <Input
          type="textarea"
          :value="generatedToken"
          readonly
          rows="8"
          class="h-auto"
        />
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="outline" class="w-full sm:w-auto" @click="handleOpenChange(false)">{{
          t("common.cancel")
        }}</Button>
        <Button v-if="!generatedToken" class="w-full sm:w-auto" @click="handleSubmit">{{
          t("issueTokenModal.issueButton")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
