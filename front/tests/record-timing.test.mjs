import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  emptyValue,
  formatDuration,
  formatMilliseconds,
  invalidValue,
} from "../src/pages/record/composables/recordFormat.ts";

const ROOT = new URL("../", import.meta.url);

test("record timing preserves zero durations and rejects reversed stages", () => {
  assert.equal(formatDuration(0, 0), "0.000 s");
  assert.equal(formatDuration(1_000, 500), invalidValue);
  assert.equal(formatDuration(null, 500), emptyValue);
  assert.equal(formatMilliseconds(0), "0 ms");
  assert.equal(formatMilliseconds(null), emptyValue);
});

test("record detail exposes distinct transport stages and semantic TTFT labels", async () => {
  const source = await readFile(
    new URL("src/pages/record/components/RecordDetailSheet.vue", ROOT),
    "utf8",
  );

  assert.match(source, /upstream_response_headers_at/);
  assert.match(source, /upstream_first_body_chunk_at/);
  assert.match(source, /first_response_body_at/);
  assert.match(source, /first_token_at/);
  assert.match(source, /max_upstream_response_idle_ms/);
  assert.match(source, /timeline\.notApplicable/);
  assert.match(source, /timeline\.notObserved/);
  assert.doesNotMatch(source, /summary\.firstByte/);
});

test("record timing labels are localized and do not use the retired first-byte metric", async () => {
  const [english, chinese] = await Promise.all([
    readFile(new URL("src/i18n/locales/en/messages.json", ROOT), "utf8").then(JSON.parse),
    readFile(new URL("src/i18n/locales/zh/messages.json", ROOT), "utf8").then(JSON.parse),
  ]);

  for (const messages of [english, chinese]) {
    assert.ok(messages.recordPage.detailDialog.summary.timeToFirstResponseBody);
    assert.ok(messages.recordPage.detailDialog.summary.ttft);
    assert.ok(messages.recordPage.detailDialog.summary.maxResponseIdle);
    assert.ok(messages.recordPage.detailDialog.timeline.notApplicable);
    assert.ok(messages.recordPage.detailDialog.timeline.notObserved);
    assert.ok(messages.recordPage.table.firstResponseBody);
    assert.ok(messages.recordPage.table.ttft);
  }
});
