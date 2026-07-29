import { computed, ref } from "vue";
import type { RecordDetail, RecordRequest } from "../../../services/types";
import { normalizeError } from "../../../utils/error.ts";
import { emptyValue } from "./recordFormat.ts";

export type RecordDetailTab = "overview";

export const RECORD_DETAIL_TABS: Array<{
  value: RecordDetailTab;
  labelKey: string;
}> = [{ value: "overview", labelKey: "recordPage.detailDialog.tabs.overview" }];

export type RecordDetailTranslator = (
  key: string,
  params?: Record<string, unknown>,
) => string;

export interface UseRecordDetailOptions {
  t: RecordDetailTranslator;
  getApiKeyName: (id: number | null) => string;
  getProviderName: (id: number | null) => string;
  api: {
    getRecordDetail: (id: number | string) => Promise<RecordDetail>;
  };
}

export function useRecordDetail(options: UseRecordDetailOptions) {
  const isDetailLoading = ref(false);
  const detailError = ref<string | null>(null);
  const detailedRecord = ref<RecordRequest | null>(null);

  const detailApiKeyName = computed(() =>
    detailedRecord.value
      ? options.getApiKeyName(detailedRecord.value.api_key_id)
      : emptyValue,
  );

  const detailProviderName = computed(() => {
    const record = detailedRecord.value;
    if (!record) return emptyValue;
    return record.provider_name || options.getProviderName(record.provider_id);
  });

  const resetDetail = () => {
    isDetailLoading.value = false;
    detailError.value = null;
    detailedRecord.value = null;
  };

  const loadDetail = async (id: number) => {
    isDetailLoading.value = true;
    detailError.value = null;
    detailedRecord.value = null;
    try {
      const detail = await options.api.getRecordDetail(id);
      detailedRecord.value = detail;
      return detail;
    } catch (err: unknown) {
      detailError.value = normalizeError(
        err,
        options.t("recordPage.detailModal.fetchFailed"),
      ).message;
      throw err;
    } finally {
      isDetailLoading.value = false;
    }
  };

  return {
    isDetailLoading,
    detailError,
    detailedRecord,
    detailApiKeyName,
    detailProviderName,
    loadDetail,
    resetDetail,
  };
}
