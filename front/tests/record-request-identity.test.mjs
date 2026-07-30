import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const ROOT = new URL("../", import.meta.url);

const readSource = (path) => readFile(new URL(path, ROOT), "utf8");

test("record contracts expose canonical and caller request identities", async () => {
  const source = await readSource("src/services/types/records.ts");

  assert.match(source, /request_id:\s*string;/);
  assert.match(source, /client_request_id:\s*string\s*\|\s*null;/);
});

test("record list shows canonical identity without adding a desktop column", async () => {
  const source = await readSource(
    "src/pages/record/components/RecordTable.vue",
  );

  assert.match(
    source,
    /recordPage\.table\.requestId[\s\S]*break-all[\s\S]*record\.request_id/,
  );
  assert.match(
    source,
    /displayRequestedModelName[\s\S]*truncate font-mono[\s\S]*record\.request_id/,
  );
  assert.doesNotMatch(
    source,
    /<TableHead[^>]*>[\s\S]{0,160}recordPage\.table\.requestId/,
  );
});

test("record detail copies the full canonical identity and labels caller identity", async () => {
  const source = await readSource(
    "src/pages/record/components/RecordDetailSheet.vue",
  );

  assert.match(source, /gatewayRequestId[\s\S]*record\.request_id/);
  assert.match(source, /const requestId = props\.record\?\.request_id/);
  assert.match(source, /await copyText\(requestId\)/);
  assert.match(source, /toastController\.success/);
  assert.match(source, /toastController\.error/);
  assert.match(source, /record\.client_request_id[\s\S]*clientRequestId/);
  assert.match(source, /detailDialog\.recordId/);
});

test("record identity labels and copy feedback stay aligned in both locales", async () => {
  const [english, chinese] = await Promise.all([
    readSource("src/i18n/locales/en/messages.json").then(JSON.parse),
    readSource("src/i18n/locales/zh/messages.json").then(JSON.parse),
  ]);

  for (const messages of [english, chinese]) {
    assert.ok(messages.recordPage.filter.searchPlaceholder.includes("ID"));
    assert.ok(messages.recordPage.table.requestId);
    assert.ok(messages.recordPage.detailDialog.recordId);
    assert.ok(messages.recordPage.detailDialog.gatewayRequestId);
    assert.ok(messages.recordPage.detailDialog.copyRequestId);
    assert.ok(messages.recordPage.detailDialog.copySuccess);
    assert.ok(messages.recordPage.detailDialog.copyFailed);
    assert.ok(messages.recordPage.detailDialog.summary.clientRequestId);
  }
});
