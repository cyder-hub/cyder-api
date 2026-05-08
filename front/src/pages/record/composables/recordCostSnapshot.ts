import type { CostDetailLine, CostSnapshot } from "../../../services/types";

export type RuntimeCostSnapshot = Omit<
  CostSnapshot,
  "detail_lines" | "unmatched_items" | "warnings"
> &
  Partial<Pick<CostSnapshot, "detail_lines" | "unmatched_items" | "warnings">>;

const arrayOrEmpty = <T>(value: T[] | null | undefined) =>
  Array.isArray(value) ? value : [];

export const costSnapshotDetailLines = (
  snapshot: RuntimeCostSnapshot | null | undefined,
): CostDetailLine[] => arrayOrEmpty(snapshot?.detail_lines);

export const costSnapshotIssueCount = (
  snapshot: RuntimeCostSnapshot | null | undefined,
) =>
  arrayOrEmpty(snapshot?.warnings).length +
  arrayOrEmpty(snapshot?.unmatched_items).length;
