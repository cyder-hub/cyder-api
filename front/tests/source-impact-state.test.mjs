import test from "node:test";
import assert from "node:assert/strict";

import { summarizeSourceImpact } from "../src/pages/provider-edit/composables/sourceImpactViewModel.ts";

test("source impact preview aggregates all protocol change counts", () => {
  assert.deepEqual(
    summarizeSourceImpact({
      action: "DELETE",
      provider_id: 1,
      source_id: 2,
      inherit_all_model_count: 3,
      explicit_binding_model_count: 4,
      explicit_default_model_count: 2,
      protocols: [
        {
          downstream_protocol: "OPENAI",
          selection_changed_count: 2,
          would_become_unselectable_count: 1,
        },
        {
          downstream_protocol: "GEMINI",
          selection_changed_count: 3,
          would_become_unselectable_count: 2,
        },
      ],
    }),
    {
      selectionChangedCount: 5,
      wouldBecomeUnselectableCount: 3,
      protocolLines: [
        {
          downstream_protocol: "OPENAI",
          selection_changed_count: 2,
          would_become_unselectable_count: 1,
        },
        {
          downstream_protocol: "GEMINI",
          selection_changed_count: 3,
          would_become_unselectable_count: 2,
        },
      ],
    },
  );
});
