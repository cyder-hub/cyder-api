import type { SourceImpactProtocolSummary, SourceImpactReport } from "@/services/types";

export interface SourceImpactSummary {
  selectionChangedCount: number;
  wouldBecomeUnselectableCount: number;
  protocolLines: SourceImpactProtocolSummary[];
}

export function summarizeSourceImpact(report: SourceImpactReport): SourceImpactSummary {
  return {
    selectionChangedCount: report.protocols.reduce(
      (total, protocol) => total + protocol.selection_changed_count,
      0,
    ),
    wouldBecomeUnselectableCount: report.protocols.reduce(
      (total, protocol) => total + protocol.would_become_unselectable_count,
      0,
    ),
    protocolLines: report.protocols,
  };
}
