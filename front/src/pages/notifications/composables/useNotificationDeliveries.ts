import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

import * as notificationService from "@/services/notifications";
import type { NotificationDelivery } from "@/services/types";
import { normalizeError } from "@/utils/error";
import type {
  DeliveryStatusFilter,
  NotificationSelectOption,
} from "../types";
import { buildNotificationDeliveryParams } from "./notificationViewModel";

export function useNotificationDeliveries() {
  const { t } = useI18n();

  const deliveries = ref<NotificationDelivery[]>([]);
  const deliveryLoading = ref(false);
  const deliveryError = ref<string | null>(null);
  const deliveryStatus = ref<DeliveryStatusFilter>("failed");
  const deliveryChannel = ref("all");

  const statusOptions = computed<
    NotificationSelectOption<DeliveryStatusFilter>[]
  >(() => [
    { value: "failed", label: t("notificationPage.delivery.status.failed") },
    {
      value: "retry_scheduled",
      label: t("notificationPage.delivery.status.retry_scheduled"),
    },
    { value: "skipped", label: t("notificationPage.delivery.status.skipped") },
    {
      value: "in_progress",
      label: t("notificationPage.delivery.status.in_progress"),
    },
    { value: "pending", label: t("notificationPage.delivery.status.pending") },
    { value: "succeeded", label: t("notificationPage.delivery.status.succeeded") },
    { value: "all", label: t("notificationPage.delivery.status.all") },
  ]);

  const loadDeliveries = async () => {
    deliveryLoading.value = true;
    deliveryError.value = null;
    try {
      const response = await notificationService.getNotificationDeliveries(
        buildNotificationDeliveryParams(
          deliveryStatus.value,
          deliveryChannel.value,
        ),
      );
      deliveries.value = response.items;
    } catch (err: unknown) {
      deliveryError.value = normalizeError(err, t("common.unknownError")).message;
      deliveries.value = [];
    } finally {
      deliveryLoading.value = false;
    }
  };

  return {
    deliveries,
    deliveryLoading,
    deliveryError,
    deliveryStatus,
    deliveryChannel,
    statusOptions,
    loadDeliveries,
  };
}
