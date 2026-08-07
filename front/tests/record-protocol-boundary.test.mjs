import test from "node:test";
import assert from "node:assert/strict";

import { useRecordList } from "../src/pages/record/composables/useRecordList.ts";
import {
  DEFAULT_RECORD_FILTERS,
  buildRecordListParams,
  buildRecordQueryFromState,
  parseRecordQueryState,
} from "../src/pages/record/composables/useRecordQuery.ts";

const filters = () => ({ ...DEFAULT_RECORD_FILTERS });

test("record query uses only the new four-value downstream protocol field", () => {
  const parsed = parseRecordQueryState({
    downstream_protocol: "RESPONSES",
    user_api_type: "OPENAI",
  });
  assert.equal(parsed.filters.downstream_protocol, "RESPONSES");

  const params = buildRecordListParams(1, 10, parsed.filters);
  assert.equal(params.downstream_protocol, "RESPONSES");
  assert.equal("user_api_type" in params, false);

  const query = buildRecordQueryFromState({
    page: 1,
    pageSize: 10,
    filters: parsed.filters,
  });
  assert.equal(query.downstream_protocol, "RESPONSES");
  assert.equal("user_api_type" in query, false);
});

test("record query rejects upstream-only and provider-dialect values", () => {
  for (const downstream_protocol of ["OLLAMA", "GEMINI_OPENAI", "UNKNOWN"]) {
    const parsed = parseRecordQueryState({ downstream_protocol });
    assert.equal(parsed.filters.downstream_protocol, "ALL");
    assert.equal(
      buildRecordListParams(1, 10, parsed.filters).downstream_protocol,
      undefined,
    );
  }
});

test("record downstream filter exposes exactly four protocols", () => {
  const state = useRecordList({
    filters: filters(),
    currentPage: { value: 1 },
    pageSize: { value: 10 },
    buildListParams: () => ({}),
    t: (key) => key,
    providerStore: {
      providers: [],
      sources: [],
      fetchProviders: async () => {},
      fetchProviderSources: async () => {},
    },
    apiKeyStore: { apiKeys: [], fetchApiKeys: async () => {} },
    modelStore: { modelOptions: [], fetchModels: async () => {} },
    api: {
      getRecordList: async () => ({
        list: [],
        total: 0,
        page: 1,
        page_size: 10,
      }),
    },
  });

  assert.deepEqual(
    state.downstreamProtocolOptions.value.map((option) => option.value),
    ["ALL", "OPENAI", "RESPONSES", "ANTHROPIC", "GEMINI"],
  );
});
