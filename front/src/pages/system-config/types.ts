import type {
  JsonValue,
  SystemConfigField,
  SystemConfigHistoryItem,
  SystemConfigLayerKind,
} from "@/services/types";
import type { SystemConfigValueDisplay } from "./composables/systemConfigState";

export type BooleanFilterKey =
  | "editable"
  | "hotReloadable"
  | "restartRequired"
  | "sensitive";

export interface FieldBadge {
  key: string;
  label: string;
  class: string;
}

export interface FieldRow {
  field: SystemConfigField;
  value: SystemConfigValueDisplay;
  badges: FieldBadge[];
}

export interface EditDraft {
  raw: string;
  boolValue: boolean;
  isNull: boolean;
}

export type DraftBuildResult =
  | { ok: true; value: JsonValue }
  | { ok: false; message: string };

export type ConfigViewMode = "effective" | "override";

export interface SystemConfigSummaryCard {
  key: "version" | "fields" | "editable" | "hotReloadable" | "override";
  label: string;
  value: string | number;
}

export interface SystemConfigSourceLayer {
  kind: SystemConfigLayerKind;
  count: number;
  configured: number;
}

export interface SystemConfigHistoryRow {
  item: SystemConfigHistoryItem;
  diff: Array<{
    path: string;
    oldText: string;
    newText: string;
  }>;
}
