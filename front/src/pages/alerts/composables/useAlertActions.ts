import { computed, ref, watch, type Ref } from "vue";
import { useI18n } from "vue-i18n";

import * as alertService from "@/services/alerts";
import { toastController } from "@/services/uiFeedback";
import type { AlertEvent } from "@/services/types";
import { normalizeError } from "@/utils/error";
import type { AlertActionKey } from "../types";
import {
  isAlertSuppressed,
  parseAlertDateTimeLocal,
  toAlertDateTimeLocal,
} from "./alertViewModel";

interface UseAlertActionsOptions {
  selectedAlert: Ref<AlertEvent | null>;
  reloadAlerts: () => Promise<void>;
}

export function useAlertActions({
  selectedAlert,
  reloadAlerts,
}: UseAlertActionsOptions) {
  const { t } = useI18n();

  const actionLoading = ref<AlertActionKey | null>(null);
  const ackNote = ref("");
  const suppressUntil = ref("");
  const suppressReason = ref("");

  const canAcknowledge = computed(
    () => !!selectedAlert.value && !selectedAlert.value.acknowledged_at,
  );
  const canSuppress = computed(
    () => !!selectedAlert.value && selectedAlert.value.status === "active",
  );
  const canUnsuppress = computed(
    () => !!selectedAlert.value && isAlertSuppressed(selectedAlert.value),
  );
  const canResolve = computed(
    () => !!selectedAlert.value && selectedAlert.value.status === "active",
  );

  watch(
    selectedAlert,
    (alert) => {
      ackNote.value = alert?.acknowledged_note ?? "";
      suppressReason.value = alert?.suppressed_reason ?? "";
      suppressUntil.value = alert?.suppressed_until
        ? toAlertDateTimeLocal(alert.suppressed_until)
        : "";
    },
    { immediate: true },
  );

  const acknowledgeSelected = async () => {
    if (!selectedAlert.value) return;
    actionLoading.value = "ack";
    try {
      selectedAlert.value = await alertService.acknowledgeAlert(
        selectedAlert.value.id,
        { note: ackNote.value.trim() || null },
      );
      toastController.success(t("alertsPage.toast.acknowledged"));
      await reloadAlerts();
    } catch (err: unknown) {
      toastController.error(
        t("alertsPage.toast.actionFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionLoading.value = null;
    }
  };

  const suppressSelected = async () => {
    if (!selectedAlert.value) return;
    const until = parseAlertDateTimeLocal(suppressUntil.value);
    if (!until || until <= Date.now()) {
      toastController.warn(t("alertsPage.toast.invalidSuppressUntil"));
      return;
    }

    actionLoading.value = "suppress";
    try {
      selectedAlert.value = await alertService.suppressAlert(
        selectedAlert.value.id,
        {
          suppressed_until: until,
          reason: suppressReason.value.trim() || null,
        },
      );
      toastController.success(t("alertsPage.toast.suppressed"));
      await reloadAlerts();
    } catch (err: unknown) {
      toastController.error(
        t("alertsPage.toast.actionFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionLoading.value = null;
    }
  };

  const unsuppressSelected = async () => {
    if (!selectedAlert.value) return;
    actionLoading.value = "unsuppress";
    try {
      selectedAlert.value = await alertService.unsuppressAlert(
        selectedAlert.value.id,
      );
      suppressUntil.value = "";
      suppressReason.value = "";
      toastController.success(t("alertsPage.toast.unsuppressed"));
      await reloadAlerts();
    } catch (err: unknown) {
      toastController.error(
        t("alertsPage.toast.actionFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionLoading.value = null;
    }
  };

  const resolveSelected = async () => {
    if (!selectedAlert.value) return;
    actionLoading.value = "resolve";
    try {
      selectedAlert.value = await alertService.resolveAlert(selectedAlert.value.id);
      toastController.success(t("alertsPage.toast.resolved"));
      await reloadAlerts();
    } catch (err: unknown) {
      toastController.error(
        t("alertsPage.toast.actionFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionLoading.value = null;
    }
  };

  return {
    actionLoading,
    ackNote,
    suppressUntil,
    suppressReason,
    canAcknowledge,
    canSuppress,
    canUnsuppress,
    canResolve,
    acknowledgeSelected,
    suppressSelected,
    unsuppressSelected,
    resolveSelected,
  };
}
