import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  formatSafeSourceBaseUrl,
  formatSourceIdentity,
} from "../src/utils/sourceEvidence.ts";
import {
  DEFAULT_RECORD_FILTERS,
  buildRecordListParams,
  parseRecordQueryState,
} from "../src/pages/record/composables/useRecordQuery.ts";

const ROOT = new URL("../", import.meta.url);
const readSource = (path) => readFile(new URL(path, ROOT), "utf8");

test("Source evidence distinguishes a selected Source from a pre-routing failure", () => {
  assert.equal(
    formatSourceIdentity(
      {
        source_id: 42,
        source_profile_type: "OPENAI",
      },
      "Source not selected",
    ),
    "#42 · OPENAI",
  );
  assert.equal(
    formatSourceIdentity(
      {
        source_id: null,
        source_profile_type: null,
      },
      "Source not selected",
    ),
    "Source not selected",
  );
});

test("Source base URL display strips credentials, query, and fragment", () => {
  assert.equal(
    formatSafeSourceBaseUrl(
      "https://user:secret@example.com/v1?api-key=secret#fragment",
      "/",
    ),
    "https://example.com/v1",
  );
  assert.equal(formatSafeSourceBaseUrl("not a url", "/"), "/");
});

test("Record Source filter round-trips to the source_id API query", () => {
  const parsed = parseRecordQueryState(
    { source_id: "42" },
    10,
    { hasSourceId: (id) => id === 42 },
  );
  assert.equal(parsed.filters.source_id, 42);
  assert.equal(buildRecordListParams(1, 10, parsed.filters).source_id, 42);
  assert.equal(
    buildRecordListParams(1, 10, DEFAULT_RECORD_FILTERS).source_id,
    undefined,
  );
});

test("Record and Runtime views render Source-first evidence without legacy Runtime fields", async () => {
  const [recordTypes, runtimeTypes, recordTable, recordDetail, runtimePage, runtimeCards, runtimeTable] =
    await Promise.all([
      readSource("src/services/types/records.ts"),
      readSource("src/services/types/providerRuntime.ts"),
      readSource("src/pages/record/components/RecordTable.vue"),
      readSource("src/pages/record/components/RecordDetailSheet.vue"),
      readSource("src/pages/provider-runtime/ProviderRuntimePage.vue"),
      readSource("src/pages/provider-runtime/components/ProviderRuntimeCards.vue"),
      readSource("src/pages/provider-runtime/components/ProviderRuntimeTable.vue"),
    ]);

  for (const field of ["source_id", "source_profile_type"]) {
    assert.match(recordTypes, new RegExp(`${field}:`));
    assert.match(runtimeTypes, new RegExp(`${field}:`));
  }
  assert.doesNotMatch(recordTypes, /source_key:/);
  assert.doesNotMatch(runtimeTypes, /source_key:/);
  assert.match(recordTypes, /source_base_url:/);
  assert.match(recordTypes, /source_selection_reason:\s*string\s*\|\s*null/);
  assert.match(runtimeTypes, /source_base_url:/);
  assert.doesNotMatch(runtimeTypes, /provider_type:/);
  assert.doesNotMatch(runtimeTypes, /\n\s*use_proxy:/);

  assert.match(recordTable, /record\.sourceDisplay/);
  assert.match(recordDetail, /formatSafeSourceBaseUrl\(record\.source_base_url/);
  assert.match(recordDetail, /record\.source_selection_reason/);
  assert.match(recordDetail, /transformLabel/);
  assert.match(recordDetail, /transformRequired/);
  assert.match(runtimePage, /source_id: String\(item\.source_id\)/);
  assert.match(runtimeCards, /providerRuntimePage\.source\.title/);
  assert.match(runtimeTable, /providerRuntimePage\.table\.runtime/);
  assert.doesNotMatch(runtimeCards, /circuitScope/);
  assert.doesNotMatch(runtimeTable, /sourceCircuit/);
  assert.doesNotMatch(runtimeCards, /\{\{\s*item\.source_base_url\s*\}\}/);
  assert.doesNotMatch(runtimeTable, /\{\{\s*item\.source_base_url\s*\}\}/);
});
