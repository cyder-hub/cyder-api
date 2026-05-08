import { computed, ref, shallowRef } from "vue";

import * as systemConfigService from "@/services/systemConfig";
import type { SystemConfigHistoryItem } from "@/services/types";
import type { SystemConfigHistoryRow } from "../types";
import { buildSystemConfigHistoryDiffDisplay } from "./systemConfigState";
import { toErrorMessage } from "./useSystemConfigReport";

const HISTORY_LIMIT = 20;

export function useSystemConfigHistory() {
  const historyItems = shallowRef<SystemConfigHistoryItem[]>([]);
  const isHistoryLoading = ref(false);
  const historyError = ref<string | null>(null);
  const historyOffset = ref(0);
  const hasMoreHistory = ref(false);

  const historyRows = computed<SystemConfigHistoryRow[]>(() =>
    historyItems.value.map((item) => ({
      item,
      diff: buildSystemConfigHistoryDiffDisplay(item.diff),
    })),
  );

  async function loadHistory(reset = false): Promise<void> {
    if (reset) {
      historyOffset.value = 0;
      historyItems.value = [];
    }
    isHistoryLoading.value = true;
    historyError.value = null;
    try {
      const items = await systemConfigService.getSystemConfigHistory({
        limit: HISTORY_LIMIT,
        offset: historyOffset.value,
      });
      historyItems.value = reset ? items : [...historyItems.value, ...items];
      historyOffset.value += items.length;
      hasMoreHistory.value = items.length === HISTORY_LIMIT;
    } catch (err: unknown) {
      historyError.value = toErrorMessage(err);
    } finally {
      isHistoryLoading.value = false;
    }
  }

  return {
    historyItems,
    historyRows,
    isHistoryLoading,
    historyError,
    historyOffset,
    hasMoreHistory,
    loadHistory,
  };
}
