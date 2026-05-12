import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  DYNAMIC_I18N_FALLBACK_EXCEPTIONS,
  DYNAMIC_I18N_KEY_SOURCES,
} from "../src/i18n/dynamic-key-candidates.ts";

const ROOT = new URL("../", import.meta.url);

const REQUIRED_KEYS = [
  "common.goHome",
  "common.pageNotFound",
  "common.pageNotFoundDescription",
  "ui.pagination.items",
  "ui.pagination.previousPage",
  "ui.pagination.nextPage",
  "providerEditPage.sections.quickStart.identityTitle",
  "providerPage.table.status",
  "providerRuntimePage.metrics.totalLatency",
  "recordPage.diagnostics.retentionFailed",
  "recordPage.diagnostics.storageInventoryFailed",
];

const RENAMED_KEYS = [
  "pagination.items",
  "pagination.previousPage",
  "pagination.nextPage",
  "providerEditPage.quickStart.identityTitle",
  "providerRuntimePage.metrics.latency",
];

function getPath(source, path) {
  return path.split(".").reduce((value, key) => value?.[key], source);
}

async function readMessages(locale) {
  const source = await readFile(
    new URL(`src/i18n/locales/${locale}/messages.json`, ROOT),
    "utf8",
  );
  return JSON.parse(source);
}

test("task 5 required i18n keys are present in every locale", async () => {
  const locales = [
    ["en", await readMessages("en")],
    ["zh", await readMessages("zh")],
  ];

  for (const [locale, messages] of locales) {
    for (const key of REQUIRED_KEYS) {
      const value = getPath(messages, key);
      assert.equal(typeof value, "string", `${locale}.${key} exists`);
      assert.notEqual(value.trim(), "", `${locale}.${key} is not empty`);
    }
  }
});

test("task 5 renamed i18n keys do not remain in messages", async () => {
  const locales = [
    ["en", await readMessages("en")],
    ["zh", await readMessages("zh")],
  ];

  for (const [locale, messages] of locales) {
    for (const key of RENAMED_KEYS) {
      assert.equal(getPath(messages, key), undefined, `${locale}.${key} is absent`);
    }
  }
});

test("dynamic i18n key candidates cover known high-risk sources", () => {
  const sourceIds = new Set(DYNAMIC_I18N_KEY_SOURCES.map((source) => source.id));

  for (const id of [
    "route-title",
    "sidebar-item",
    "request-patch-prefix",
    "alerts",
    "notifications",
    "api-key-governance",
    "cost-options",
    "portable-config-enums",
    "record-detail-tabs",
  ]) {
    assert.equal(sourceIds.has(id), true, `${id} dynamic key source exists`);
  }

  assert.equal(
    DYNAMIC_I18N_FALLBACK_EXCEPTIONS.some(
      (exception) => exception.id === "record-replay-unavailable-reason",
    ),
    true,
  );
});
