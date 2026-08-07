import test from "node:test";
import assert from "node:assert/strict";

import {
  applySourceSelectionMode,
  clearSourceDefault,
  createSourceConfigDraft,
  setSourceDefault,
  sourceConfigPayloadEquals,
  toSourceConfigPayload,
  toggleSourceBinding,
  visibleSourceOptions,
} from "../src/components/model-source-config/sourceConfigViewModel.ts";

const sources = [
  {
    id: 20,
    profile_type: "OLLAMA",
    endpoint: "http://ollama",
    use_proxy: false,
    is_enabled: false,
    is_default: false,
    deleted_at: null,
  },
  {
    id: 10,
    profile_type: "OPENAI",
    endpoint: "http://openai",
    use_proxy: false,
    is_enabled: true,
    is_default: true,
    deleted_at: null,
  },
  {
    id: 30,
    profile_type: "GEMINI",
    endpoint: "http://deleted",
    use_proxy: false,
    is_enabled: true,
    is_default: false,
    deleted_at: 123,
  },
];

test("source config hydrate hides deleted bindings and keeps explicit defaults", () => {
  const draft = createSourceConfigDraft(
    {
      source_selection_mode: "EXPLICIT",
      bindings: [
        { source_id: 20, is_default: true, profile_type: "OLLAMA", is_enabled: false },
        { source_id: 30, is_default: false, profile_type: "GEMINI", is_enabled: true },
      ],
      declared_source_count: 2,
      enabled_source_count: 1,
      model_default_source_id: 20,
      warnings: [],
    },
    sources,
  );

  assert.deepEqual(draft, {
    source_selection_mode: "EXPLICIT",
    bindings: [{ source_id: 20, is_default: true }],
  });
  assert.deepEqual(
    visibleSourceOptions(sources).map((source) => source.id),
    [10, 20],
  );
});

test("mode switches preserve the inherit contract and seed explicit sources deterministically", () => {
  const inherited = createSourceConfigDraft({
    source_selection_mode: "INHERIT_ALL",
    bindings: [],
    declared_source_count: 2,
    enabled_source_count: 1,
    model_default_source_id: null,
    warnings: [],
  }, sources);
  const explicit = applySourceSelectionMode(inherited, "EXPLICIT", sources);

  assert.deepEqual(toSourceConfigPayload(explicit), {
    source_selection_mode: "EXPLICIT",
    bindings: [
      { source_id: 10, is_default: true },
      { source_id: 20, is_default: false },
    ],
  });
  assert.deepEqual(applySourceSelectionMode(explicit, "INHERIT_ALL", sources), {
    source_selection_mode: "INHERIT_ALL",
    bindings: [],
  });
});

test("explicit selection supports add/remove and a single default", () => {
  let draft = {
    source_selection_mode: "EXPLICIT",
    bindings: [{ source_id: 10, is_default: true }],
  };
  draft = toggleSourceBinding(draft, 20, true);
  draft = setSourceDefault(draft, 20);
  assert.deepEqual(draft.bindings, [
    { source_id: 10, is_default: false },
    { source_id: 20, is_default: true },
  ]);

  draft = toggleSourceBinding(draft, 20, false);
  assert.deepEqual(draft.bindings, [{ source_id: 10, is_default: false }]);
  assert.equal(
    sourceConfigPayloadEquals(draft, {
      source_selection_mode: "EXPLICIT",
      bindings: [{ source_id: 10, is_default: false }],
    }),
    true,
  );
});

test("an explicit model default can be cleared without removing its binding", () => {
  const draft = clearSourceDefault({
    source_selection_mode: "EXPLICIT",
    bindings: [
      { source_id: 10, is_default: true },
      { source_id: 20, is_default: false },
    ],
  });

  assert.deepEqual(draft.bindings, [
    { source_id: 10, is_default: false },
    { source_id: 20, is_default: false },
  ]);
});
