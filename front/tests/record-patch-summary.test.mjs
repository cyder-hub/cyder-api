import test from "node:test";
import assert from "node:assert/strict";

import {
  patchSourceItemsFromRaw,
  reasoningSourceDetail,
} from "../src/components/record/recordPatchSummary.ts";

const translate = (key, params = {}) => {
  switch (key) {
    case "recordPage.detailDialog.attempts.patchSources.providerRule":
      return `provider rule #${params.id}`;
    case "recordPage.detailDialog.attempts.patchSources.modelRule":
      return `model rule #${params.id}`;
    case "recordPage.detailDialog.attempts.patchSources.reasoningPreset":
      return `reasoning ${params.preset} (-${params.suffix}) / ${params.family} / ${params.source}`;
    case "recordPage.detailDialog.attempts.patchSources.unknown":
      return `unknown ${params.kind}`;
    default:
      return key;
  }
};

test("patch source summary labels config-oriented reasoning preset sources", () => {
  const labels = patchSourceItemsFromRaw(
    {
      effective_rules: [
        {
          source: {
            kind: "reasoning_preset",
            config_id: 101,
            config_scope: "model",
            config_preset_id: 202,
            family: "openai_chat_reasoning_effort",
            preset: "high",
            suffix: "high",
          },
          overridden_sources: [{ kind: "provider_rule", rule_id: 7 }],
        },
      ],
      conflicts: [
        {
          lower_priority_source: { kind: "provider_rule", rule_id: 7 },
          higher_priority_source: { kind: "model_rule", rule_id: 8 },
        },
      ],
    },
    translate,
  );

  assert.deepEqual(labels, [
    "reasoning high (-high) / openai_chat_reasoning_effort / config model/101 preset row 202",
    "provider rule #7",
    "model rule #8",
  ]);
});

test("patch source summary keeps legacy reasoning profile sources readable", () => {
  assert.equal(
    reasoningSourceDetail({
      kind: "reasoning_preset",
      profile_id: 11,
      profile_preset_id: 12,
    }),
    "legacy reasoning preset profile 11 preset row 12",
  );
});

test("patch source summary ignores malformed summaries", () => {
  assert.deepEqual(patchSourceItemsFromRaw("{not-json", translate), []);
  assert.deepEqual(patchSourceItemsFromRaw([], translate), []);
});
