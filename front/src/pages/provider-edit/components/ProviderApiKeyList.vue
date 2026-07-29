<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader :title="$t('providerEditPage.sectionApiKeys')">
      <template #meta>
        <p class="mt-1 text-xs text-gray-500">
          {{ editingData.provider_keys.length }} {{ $t("providerEditPage.credentials.items") }}
        </p>
      </template>
      <template #actions>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
          <Button
            variant="outline"
            size="sm"
            :disabled="!editingData.id || editingData.provider_keys.length === 0"
            @click="emit('checkBatch')"
          >
            <Check class="mr-1.5 h-4 w-4" />
            {{ $t("providerEditPage.alert.buttonCheckAll") }}
          </Button>
          <Button size="sm" :disabled="!editingData.id" @click="openCreateDialog">
            <Plus class="mr-1.5 h-4 w-4" />
            {{ $t("providerEditPage.buttonAddApiKey") }}
          </Button>
        </div>
      </template>
    </SectionHeader>

    <div
      v-if="editingData.provider_keys.length === 0"
      class="flex flex-col items-center justify-center rounded-xl border border-dashed border-gray-200 py-10"
    >
      <KeyRound class="mb-2 h-9 w-9 stroke-1 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">
        {{ $t("providerEditPage.alert.noApiKeys") }}
      </span>
    </div>

    <div v-else class="divide-y divide-gray-100 overflow-hidden rounded-lg border border-gray-200">
      <div
        v-for="keyItem in editingData.provider_keys"
        :key="keyItem.id"
        class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between"
      >
        <div class="min-w-0 space-y-1">
          <div class="flex flex-wrap items-center gap-2">
            <code class="rounded bg-gray-100 px-2 py-1 text-xs text-gray-800">
              {{ keyMask(keyItem) }}
            </code>
            <Badge :variant="keyItem.is_enabled ? 'secondary' : 'outline'">
              {{
                $t(
                  keyItem.is_enabled
                    ? "providerEditPage.credentials.enabled"
                    : "providerEditPage.credentials.disabled",
                )
              }}
            </Badge>
          </div>
          <p class="truncate text-sm text-gray-600">
            {{ keyItem.description || $t("providerEditPage.credentials.noDescription") }}
          </p>
          <p v-if="keyItem.checkMessage" class="text-xs text-red-600">
            {{ keyItem.checkMessage }}
          </p>
        </div>

        <div class="grid grid-cols-2 gap-2 sm:flex sm:flex-wrap sm:justify-end">
          <Button
            variant="outline"
            size="sm"
            :disabled="busyKeyId === keyItem.id"
            @click="emit('checkSingle', editingData.provider_keys.indexOf(keyItem))"
          >
            <Loader2 v-if="keyItem.checkStatus === 'checking'" class="mr-1.5 h-4 w-4 animate-spin" />
            <Check v-else class="mr-1.5 h-4 w-4" />
            {{ $t("common.check") }}
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="busyKeyId === keyItem.id"
            @click="openRevealDialog(keyItem)"
          >
            <Eye class="mr-1.5 h-4 w-4" />
            {{ $t("providerEditPage.credentials.reveal") }}
          </Button>
          <Button variant="outline" size="sm" @click="openReplaceDialog(keyItem)">
            <RefreshCw class="mr-1.5 h-4 w-4" />
            {{ $t("providerEditPage.credentials.replace") }}
          </Button>
          <Button variant="outline" size="sm" @click="openMetadataDialog(keyItem)">
            <Pencil class="mr-1.5 h-4 w-4" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            :disabled="busyKeyId === keyItem.id"
            @click="handleToggle(keyItem)"
          >
            <Power class="mr-1.5 h-4 w-4" />
            {{
              $t(
                keyItem.is_enabled
                  ? "providerEditPage.credentials.disable"
                  : "providerEditPage.credentials.enable",
              )
            }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="text-red-600 hover:bg-red-50 hover:text-red-700"
            :disabled="busyKeyId === keyItem.id"
            @click="handleDelete(keyItem)"
          >
            <Trash2 class="mr-1.5 h-4 w-4" />
            {{ $t("common.delete") }}
          </Button>
        </div>
      </div>
    </div>

    <Dialog :open="secretDialogOpen" @update:open="handleSecretDialogOpen">
      <DialogContent class="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>
            {{
              $t(
                secretDialogMode === "create"
                  ? "providerEditPage.credentials.createTitle"
                  : "providerEditPage.credentials.replaceTitle",
              )
            }}
          </DialogTitle>
          <DialogDescription>
            {{ $t("providerEditPage.credentials.secretLocalOnly") }}
          </DialogDescription>
        </DialogHeader>
        <div class="space-y-4">
          <div class="space-y-1.5">
            <Label>{{ $t("providerEditPage.tableHeaderApiKey") }}</Label>
            <textarea
              v-if="isVertex"
              v-model="secretInput"
              rows="9"
              class="flex w-full resize-y rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none focus:border-gray-400"
              :placeholder="$t('providerEditPage.credentials.vertexPlaceholder')"
            />
            <Input
              v-else
              v-model="secretInput"
              type="password"
              class="font-mono"
              :placeholder="$t('providerEditPage.placeholderApiKey')"
            />
            <p v-if="isVertex" class="text-xs leading-5 text-gray-500">
              {{ $t("providerEditPage.credentials.vertexHelp") }}
            </p>
          </div>
          <div v-if="secretDialogMode === 'create'" class="space-y-1.5">
            <Label>{{ $t("providerEditPage.tableHeaderDescription") }}</Label>
            <Input v-model="createDescription" :placeholder="$t('providerEditPage.placeholderDescription')" />
          </div>
        </div>
        <DialogFooter class="gap-2 sm:gap-0">
          <Button variant="ghost" @click="closeSecretDialog">{{ $t("common.cancel") }}</Button>
          <Button
            v-if="secretDialogMode === 'create'"
            variant="outline"
            :disabled="isBusy"
            @click="handleDraftCheck"
          >
            {{ $t("providerEditPage.credentials.checkDraft") }}
          </Button>
          <Button :disabled="isBusy" @click="handleSecretSubmit">
            <Loader2 v-if="isBusy" class="mr-1.5 h-4 w-4 animate-spin" />
            {{
              $t(
                secretDialogMode === "create"
                  ? "providerEditPage.credentials.create"
                  : "common.save",
              )
            }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="metadataDialogOpen" @update:open="handleMetadataDialogOpen">
      <DialogContent class="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{{ $t("providerEditPage.credentials.editTitle") }}</DialogTitle>
        </DialogHeader>
        <div class="space-y-4">
          <div class="space-y-1.5">
            <Label>{{ $t("providerEditPage.tableHeaderDescription") }}</Label>
            <Input v-model="metadataDescription" :placeholder="$t('providerEditPage.placeholderDescription')" />
          </div>
          <label class="flex items-center justify-between rounded-lg border border-gray-200 p-3 text-sm text-gray-700">
            {{ $t("providerEditPage.credentials.enabled") }}
            <Checkbox v-model="metadataEnabled" />
          </label>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="closeMetadataDialog">{{ $t("common.cancel") }}</Button>
          <Button :disabled="isBusy" @click="handleMetadataSubmit">{{ $t("common.save") }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog
      :open="revealTargetKey !== null || revealedSecret !== null"
      @update:open="handleRevealDialogOpen"
    >
      <DialogContent class="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{{ $t("providerEditPage.credentials.revealTitle") }}</DialogTitle>
          <DialogDescription>
            {{
              revealedSecret
                ? $t("providerEditPage.credentials.secretLocalOnly")
                : $t("providerEditPage.credentials.revealConfirm")
            }}
          </DialogDescription>
        </DialogHeader>
        <textarea
          v-if="revealedSecret"
          readonly
          rows="7"
          class="flex w-full resize-none rounded-md border border-gray-200 bg-gray-50 px-3 py-2 font-mono text-sm text-gray-900 outline-none"
          :value="revealedSecret.api_key"
        />
        <DialogFooter>
          <template v-if="revealTargetKey && !revealedSecret">
            <Button
              variant="ghost"
              :disabled="isRevealBusy"
              @click="clearReveal"
            >
              {{ $t("common.cancel") }}
            </Button>
            <Button
              :disabled="isRevealBusy"
              @click="handleReveal"
            >
              <Loader2 v-if="isRevealBusy" class="mr-1.5 h-4 w-4 animate-spin" />
              {{ $t("providerEditPage.credentials.reveal") }}
            </Button>
          </template>
          <Button v-else @click="clearReveal">{{ $t("common.close") }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import * as providerService from "@/services/providers";
import {
  isManagerReauthCancelled,
  runWithSecretGovernanceReauth,
} from "@/services/managerReauth";
import { toastController } from "@/services/uiFeedback";
import { useAuthStore } from "@/store/authStore";
import {
  Check,
  Eye,
  KeyRound,
  Loader2,
  Pencil,
  Plus,
  Power,
  RefreshCw,
  Trash2,
} from "lucide-vue-next";

import { mapProviderApiKeySummary } from "../composables/providerEditState";
import { useProviderCredentialSecretState } from "../composables/useProviderCredentialSecretState";
import type { EditingProviderData, LocalProviderApiKeyItem } from "../types";

const { t: $t } = useI18n();
const authStore = useAuthStore();
const editingData = defineModel<EditingProviderData>("editingData", { required: true });
const emit = defineEmits<{
  (event: "checkSingle", index: number): void;
  (event: "checkBatch"): void;
}>();

const secretDialogOpen = ref(false);
const secretDialogMode = ref<"create" | "replace">("create");
const secretState = useProviderCredentialSecretState();
const secretInput = secretState.draftSecret;
const replacementKeyId = secretState.replacementKeyId;
const revealedSecret = secretState.revealedSecret;
const createDescription = ref("");
const metadataDialogOpen = ref(false);
const metadataKeyId = ref<number | null>(null);
const metadataDescription = ref("");
const metadataEnabled = ref(true);
const isBusy = ref(false);
const busyKeyId = ref<number | null>(null);
const revealTargetKey = ref<LocalProviderApiKeyItem | null>(null);
const isRevealBusy = ref(false);

const isVertex = computed(() =>
  ["VERTEX", "VERTEX_OPENAI"].includes(editingData.value.provider_type),
);

const keyMask = (key: LocalProviderApiKeyItem) =>
  `${key.key_prefix}••••${key.key_last4}`;

const clearSecretState = () => {
  secretState.clearDialog();
  createDescription.value = "";
  secretDialogOpen.value = false;
};

const openCreateDialog = () => {
  clearSecretState();
  secretState.openCreate();
  secretDialogMode.value = "create";
  secretDialogOpen.value = true;
};

const openReplaceDialog = (key: LocalProviderApiKeyItem) => {
  clearSecretState();
  secretState.openReplace(key.id);
  secretDialogMode.value = "replace";
  secretDialogOpen.value = true;
};

const closeSecretDialog = () => clearSecretState();
const handleSecretDialogOpen = (open: boolean) => {
  if (!open) closeSecretDialog();
};

const clearReveal = () => {
  revealTargetKey.value = null;
  secretState.setRevealed(null);
};
const handleRevealDialogOpen = (open: boolean) => {
  if (!open && !isRevealBusy.value) clearReveal();
};

const validateSecret = (): boolean => {
  if (!secretInput.value.trim()) {
    toastController.warn($t("providerEditPage.alert.apiKeyRequired"));
    return false;
  }
  if (!isVertex.value) return true;
  try {
    const parsed = JSON.parse(secretInput.value) as Record<string, unknown>;
    const required = ["client_email", "private_key", "private_key_id", "token_uri"];
    if (required.some((field) => !parsed[field])) throw new Error("missing field");
  } catch {
    toastController.warn($t("providerEditPage.credentials.vertexInvalid"));
    return false;
  }
  return true;
};

const replaceLocalSummary = (summary: Awaited<ReturnType<typeof providerService.updateProviderKey>>) => {
  const next = mapProviderApiKeySummary(summary);
  if (!next) return;
  const index = editingData.value.provider_keys.findIndex((key) => key.id === next.id);
  const previous = editingData.value.provider_keys[index];
  if (index >= 0) {
    next.checkStatus = previous.checkStatus;
    next.checkMessage = previous.checkMessage;
    editingData.value.provider_keys.splice(index, 1, next);
  } else {
    editingData.value.provider_keys.push(next);
  }
};

const refreshProviderKeys = async () => {
  if (!editingData.value.id) return;
  const summaries = await providerService.getProviderKeys(editingData.value.id);
  editingData.value.provider_keys = summaries
    .map(mapProviderApiKeySummary)
    .filter((key): key is LocalProviderApiKeyItem => key !== null);
};

const recoverAfterMutationFailure = async (error: unknown) => {
  await refreshProviderKeys().catch(() => undefined);
  toastController.error(
    $t("providerEditPage.credentials.mutationFailed", {
      error: (error as Error).message || $t("common.unknownError"),
    }),
  );
};

const handleSecretSubmit = async () => {
  const providerId = editingData.value.id;
  if (!providerId || !validateSecret() || isBusy.value) return;
  isBusy.value = true;
  try {
    if (secretDialogMode.value === "create") {
      replaceLocalSummary(
        await providerService.createProviderKey(providerId, {
          api_key: secretInput.value,
          description: createDescription.value.trim() || null,
        }),
      );
    } else if (replacementKeyId.value !== null) {
      replaceLocalSummary(
        await providerService.replaceProviderKey(providerId, replacementKeyId.value, {
          api_key: secretInput.value,
        }),
      );
    }
    toastController.success($t("providerEditPage.credentials.mutationSuccess"));
    closeSecretDialog();
  } catch (error) {
    await recoverAfterMutationFailure(error);
  } finally {
    secretInput.value = "";
    isBusy.value = false;
  }
};

const handleDraftCheck = async () => {
  const providerId = editingData.value.id;
  const model = editingData.value.models.find((item) => item.id !== null);
  if (!providerId || !model || !validateSecret() || isBusy.value) {
    if (!model) toastController.warn($t("providerEditPage.alert.noModelForCheck"));
    return;
  }
  isBusy.value = true;
  try {
    await providerService.checkProviderConnection(providerId, {
      model_id: model.id ?? undefined,
      provider_api_key: secretInput.value,
    });
    toastController.success($t("providerEditPage.alert.checkSuccess"));
  } catch (error) {
    toastController.error(
      $t("providerEditPage.alert.checkFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  } finally {
    isBusy.value = false;
  }
};

const performReveal = async (key: LocalProviderApiKeyItem) => {
  if (!editingData.value.id || !key || isRevealBusy.value) return;
  isRevealBusy.value = true;
  busyKeyId.value = key.id;
  try {
    secretState.setRevealed(
      await runWithSecretGovernanceReauth(() =>
        providerService.revealProviderKey(
          editingData.value.id as number,
          key.id,
        ),
      ),
    );
    revealTargetKey.value = null;
  } catch (error) {
    if (isManagerReauthCancelled(error)) return;
    const fallback = $t("providerEditPage.credentials.revealFailed", {
      error: (error as Error).message || $t("common.unknownError"),
    });
    toastController.error(fallback);
  } finally {
    isRevealBusy.value = false;
    busyKeyId.value = null;
  }
};

const openRevealDialog = (key: LocalProviderApiKeyItem) => {
  clearReveal();
  revealTargetKey.value = key;
};

const handleReveal = async () => {
  const key = revealTargetKey.value;
  if (key) await performReveal(key);
};

const openMetadataDialog = (key: LocalProviderApiKeyItem) => {
  metadataKeyId.value = key.id;
  metadataDescription.value = key.description ?? "";
  metadataEnabled.value = key.is_enabled;
  metadataDialogOpen.value = true;
};

const closeMetadataDialog = () => {
  metadataDialogOpen.value = false;
  metadataDescription.value = "";
  metadataKeyId.value = null;
};
const handleMetadataDialogOpen = (open: boolean) => {
  if (!open) closeMetadataDialog();
};

const updateMetadata = async (keyId: number, description: string | null, enabled: boolean) => {
  const providerId = editingData.value.id;
  if (!providerId) return;
  busyKeyId.value = keyId;
  try {
    replaceLocalSummary(
      await providerService.updateProviderKey(providerId, keyId, {
        description,
        is_enabled: enabled,
      }),
    );
    toastController.success($t("providerEditPage.credentials.mutationSuccess"));
  } catch (error) {
    await recoverAfterMutationFailure(error);
  } finally {
    busyKeyId.value = null;
  }
};

const handleMetadataSubmit = async () => {
  if (metadataKeyId.value === null || isBusy.value) return;
  isBusy.value = true;
  await updateMetadata(
    metadataKeyId.value,
    metadataDescription.value.trim() || null,
    metadataEnabled.value,
  );
  isBusy.value = false;
  closeMetadataDialog();
};

const handleToggle = async (key: LocalProviderApiKeyItem) => {
  await updateMetadata(key.id, key.description, !key.is_enabled);
};

const handleDelete = async (key: LocalProviderApiKeyItem) => {
  const providerId = editingData.value.id;
  if (!providerId) return;
  busyKeyId.value = key.id;
  try {
    await providerService.deleteProviderKey(providerId, key.id);
    editingData.value.provider_keys = editingData.value.provider_keys.filter(
      (item) => item.id !== key.id,
    );
    toastController.success($t("providerEditPage.alert.apiKeyDeleteSuccess"));
  } catch (error) {
    await recoverAfterMutationFailure(error);
  } finally {
    busyKeyId.value = null;
  }
};

watch(
  () => editingData.value.id,
  () => {
    clearReveal();
    secretState.providerChanged();
    clearSecretState();
  },
);
watch(
  () => authStore.lifecycle,
  (lifecycle) => {
    if (lifecycle === "anonymous") {
      clearReveal();
      secretState.logout();
      clearSecretState();
    }
  },
);
onBeforeUnmount(() => {
  clearReveal();
  secretState.leaveRoute();
  clearSecretState();
});
</script>
