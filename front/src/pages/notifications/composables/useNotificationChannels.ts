import { ref } from "vue";
import { useI18n } from "vue-i18n";

import * as notificationService from "@/services/notifications";
import { confirm, toastController } from "@/services/uiFeedback";
import type { NotificationChannel } from "@/services/types";
import { normalizeError } from "@/utils/error";
import type { ChannelDraft } from "../types";
import {
  createNotificationChannelDraft,
  emptyNotificationChannelDraft,
  normalizeNotificationCooldownDraft,
  normalizeNotificationHeadersDraft,
} from "./notificationViewModel";

interface UseNotificationChannelsOptions {
  afterMutation?: () => Promise<void>;
}

export function useNotificationChannels(
  options: UseNotificationChannelsOptions = {},
) {
  const { t } = useI18n();

  const channels = ref<NotificationChannel[]>([]);
  const isLoading = ref(true);
  const isRefreshing = ref(false);
  const saveLoading = ref(false);
  const testLoadingId = ref<number | null>(null);
  const error = ref<string | null>(null);
  const isChannelDialogOpen = ref(false);
  const editingChannel = ref<NotificationChannel | null>(null);
  const draft = ref<ChannelDraft>(emptyNotificationChannelDraft());

  const loadChannels = async () => {
    isRefreshing.value = true;
    error.value = null;
    try {
      channels.value = await notificationService.getNotificationChannels();
    } catch (err: unknown) {
      error.value = normalizeError(err, t("common.unknownError")).message;
      throw err;
    } finally {
      isLoading.value = false;
      isRefreshing.value = false;
    }
  };

  const reloadAfterMutation = async () => {
    await loadChannels();
    await options.afterMutation?.();
  };

  const openCreateDialog = () => {
    editingChannel.value = null;
    draft.value = emptyNotificationChannelDraft();
    isChannelDialogOpen.value = true;
  };

  const openEditDialog = (channel: NotificationChannel) => {
    editingChannel.value = channel;
    draft.value = createNotificationChannelDraft(channel);
    isChannelDialogOpen.value = true;
  };

  const saveChannel = async () => {
    const trimmedName = draft.value.name.trim();
    const trimmedUrl = draft.value.endpoint_url.trim();
    const headers = normalizeNotificationHeadersDraft(draft.value.headers_json);
    if (!headers.valid) {
      toastController.warn(t("notificationPage.toast.headersInvalid"));
      return;
    }

    const cooldown = normalizeNotificationCooldownDraft(
      draft.value.cooldown_seconds,
    );
    if (!cooldown.valid) {
      toastController.warn(t("notificationPage.toast.cooldownInvalid"));
      return;
    }

    if (!trimmedName || !trimmedUrl) {
      toastController.warn(t("notificationPage.toast.required"));
      return;
    }

    saveLoading.value = true;
    try {
      if (editingChannel.value) {
        await notificationService.updateNotificationChannel(
          editingChannel.value.id,
          {
            name: trimmedName,
            endpoint_url: trimmedUrl,
            signing_secret: draft.value.signing_secret.trim() || undefined,
            clear_signing_secret: draft.value.clear_signing_secret,
            headers_json: headers.value ?? undefined,
            clear_headers: draft.value.clear_headers,
            cooldown_seconds: cooldown.value,
            is_enabled: draft.value.is_enabled,
          },
        );
        toastController.success(t("notificationPage.toast.updated"));
      } else {
        const channelKey = draft.value.channel_key.trim();
        if (!channelKey) {
          toastController.warn(t("notificationPage.toast.channelKeyRequired"));
          return;
        }
        await notificationService.createNotificationChannel({
          channel_key: channelKey,
          name: trimmedName,
          endpoint_url: trimmedUrl,
          signing_secret: draft.value.signing_secret.trim() || undefined,
          headers_json: headers.value ?? undefined,
          cooldown_seconds: cooldown.value,
          is_enabled: draft.value.is_enabled,
        });
        toastController.success(t("notificationPage.toast.created"));
      }

      isChannelDialogOpen.value = false;
      await reloadAfterMutation();
    } catch (err: unknown) {
      toastController.error(
        t("notificationPage.toast.saveFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      saveLoading.value = false;
    }
  };

  const deleteChannel = async (channel: NotificationChannel) => {
    if (
      !(await confirm(t("notificationPage.confirmDelete", { name: channel.name })))
    ) {
      return;
    }
    try {
      await notificationService.deleteNotificationChannel(channel.id);
      toastController.success(t("notificationPage.toast.deleted"));
      await reloadAfterMutation();
    } catch (err: unknown) {
      toastController.error(
        t("notificationPage.toast.deleteFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    }
  };

  const testChannel = async (channel: NotificationChannel) => {
    testLoadingId.value = channel.id;
    try {
      const result = await notificationService.testNotificationChannel(channel.id);
      if (result.success) {
        toastController.success(t("notificationPage.toast.testSucceeded"));
      } else {
        toastController.error(result.error || t("notificationPage.toast.testFailed"));
      }
      await reloadAfterMutation();
    } catch (err: unknown) {
      toastController.error(
        t("notificationPage.toast.testFailedWithError", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      testLoadingId.value = null;
    }
  };

  return {
    channels,
    isLoading,
    isRefreshing,
    saveLoading,
    testLoadingId,
    error,
    isChannelDialogOpen,
    editingChannel,
    draft,
    loadChannels,
    openCreateDialog,
    openEditDialog,
    saveChannel,
    deleteChannel,
    testChannel,
  };
}
