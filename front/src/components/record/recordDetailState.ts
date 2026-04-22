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
