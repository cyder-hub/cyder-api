import { computed, ref } from "vue";
import type {
  RecordDetail,
  RecordArtifactResponse,
  RecordAttempt,
  RecordRequest,
} from "../../../services/types";
import { normalizeError } from "../../../utils/error.ts";
import { emptyValue } from "./recordFormat.ts";

export type RecordDetailTab =
  | "overview"
  | "attempts"
  | "diagnostics"
  | "payloads"
  | "replay";

export const RECORD_DETAIL_TABS: Array<{ value: RecordDetailTab; labelKey: string }> = [
  { value: "overview", labelKey: "recordPage.detailDialog.tabs.overview" },
  { value: "attempts", labelKey: "recordPage.detailDialog.tabs.attempts" },
  { value: "diagnostics", labelKey: "recordPage.detailDialog.tabs.diagnostics" },
  { value: "payloads", labelKey: "recordPage.detailDialog.tabs.payloads" },
  { value: "replay", labelKey: "recordPage.detailDialog.tabs.replay" },
];

export const shouldLoadRecordArtifacts = (tab: RecordDetailTab) =>
  tab === "diagnostics" || tab === "replay";

export const shouldRenderPayloadViewer = (
  tab: RecordDetailTab,
  bundleStorageType: string | null | undefined,
) => tab === "payloads" && Boolean(bundleStorageType);

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
    getRecordArtifacts: (id: number | string) => Promise<RecordArtifactResponse>;
  };
}

const normalizeErrorMessage = (err: unknown) => {
  if (err instanceof Error) return err.message;
  if (typeof err === "object" && err !== null && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
};

export function useRecordDetail(options: UseRecordDetailOptions) {
  const api = options.api;
  const isDetailLoading = ref(false);
  const detailError = ref<string | null>(null);
  const detailedRecord = ref<RecordRequest | null>(null);
  const detailedAttempts = ref<RecordAttempt[]>([]);
  const artifacts = ref<RecordArtifactResponse | null>(null);
  const artifactsLoading = ref(false);
  const artifactsError = ref<string | null>(null);

  const detailApiKeyName = computed(() =>
    detailedRecord.value
      ? options.getApiKeyName(detailedRecord.value.api_key_id)
      : emptyValue,
  );

  const detailProviderName = computed(() => {
    const record = detailedRecord.value;
    if (!record) return emptyValue;
    return (
      record.final_provider_name_snapshot ||
      options.getProviderName(record.final_provider_id)
    );
  });

  const resetArtifacts = () => {
    artifacts.value = null;
    artifactsError.value = null;
  };

  const resetDetail = () => {
    isDetailLoading.value = false;
    detailError.value = null;
    detailedRecord.value = null;
    detailedAttempts.value = [];
    resetArtifacts();
  };

  const loadDetail = async (id: number) => {
    isDetailLoading.value = true;
    detailError.value = null;
    detailedRecord.value = null;
    detailedAttempts.value = [];
    resetArtifacts();

    try {
      const detail = await api.getRecordDetail(id);
      detailedRecord.value = detail.request;
      detailedAttempts.value = [...(detail.attempts ?? [])].sort(
        (left, right) => left.attempt_index - right.attempt_index,
      );
      return detail;
    } catch (err: unknown) {
      const error = normalizeError(err, options.t("recordPage.detailModal.fetchFailed"));
      detailError.value = error.message;
      throw err;
    } finally {
      isDetailLoading.value = false;
    }
  };

  const loadArtifacts = async (force = false) => {
    const record = detailedRecord.value;
    if (!record || artifactsLoading.value) return;
    if (artifacts.value && !force) return;
    artifactsLoading.value = true;
    artifactsError.value = null;
    try {
      artifacts.value = await api.getRecordArtifacts(record.id);
    } catch (err) {
      artifactsError.value = normalizeErrorMessage(err);
    } finally {
      artifactsLoading.value = false;
    }
  };

  return {
    isDetailLoading,
    detailError,
    detailedRecord,
    detailedAttempts,
    detailApiKeyName,
    detailProviderName,
    artifacts,
    artifactsLoading,
    artifactsError,
    loadDetail,
    loadArtifacts,
    resetDetail,
    resetArtifacts,
  };
}
