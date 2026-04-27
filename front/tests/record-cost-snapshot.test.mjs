import test from "node:test";
import assert from "node:assert/strict";

import {
  costSnapshotDetailLines,
  costSnapshotIssueCount,
} from "../src/components/record/recordCostSnapshot.ts";

test("cost snapshot issue count tolerates missing legacy arrays", () => {
  assert.equal(costSnapshotIssueCount(null), 0);
  assert.equal(costSnapshotIssueCount({ warnings: ["rated with fallback"] }), 1);
  assert.equal(costSnapshotIssueCount({ unmatched_items: ["llm.output_audio_tokens"] }), 1);
  assert.equal(
    costSnapshotIssueCount({
      warnings: ["rated with fallback", "missing component"],
      unmatched_items: ["llm.output_audio_tokens"],
    }),
    3,
  );
});

test("cost snapshot detail lines fall back to an empty list for old snapshots", () => {
  assert.deepEqual(costSnapshotDetailLines({}), []);

  const lines = [
    {
      meter_key: "llm.output_text_tokens",
      quantity: 128,
      unit: "1k_tokens",
      charge_kind: "usage",
      amount_nanos: 1000,
      unit_price_nanos: 8000,
      component_id: 1,
      catalog_version_id: 2,
      description: null,
    },
  ];

  assert.equal(costSnapshotDetailLines({ detail_lines: lines }), lines);
});
