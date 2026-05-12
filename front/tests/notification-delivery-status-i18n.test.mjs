import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const ROOT = new URL("../", import.meta.url);

const DELIVERY_STATUSES = [
  "pending",
  "in_progress",
  "retry_scheduled",
  "succeeded",
  "failed",
  "skipped",
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

const messagesByLocale = await Promise.all([
  readMessages("en"),
  readMessages("zh"),
]);

test("alert and notification pages translate every delivery status", () => {
  for (const [locale, dict] of [
    ["en", messagesByLocale[0]],
    ["zh", messagesByLocale[1]],
  ]) {
    for (const namespace of [
      "alertsPage.delivery.status",
      "notificationPage.delivery.status",
    ]) {
      const messages = getPath(dict, namespace);
      assert.equal(typeof messages, "object", `${locale}.${namespace} exists`);

      for (const status of DELIVERY_STATUSES) {
        assert.equal(
          typeof messages[status],
          "string",
          `${locale}.${namespace}.${status} exists`,
        );
        assert.notEqual(
          messages[status],
          `${namespace}.${status}`,
          `${locale}.${namespace}.${status} is not a raw i18n key`,
        );
        assert.notEqual(
          messages[status].trim(),
          "",
          `${locale}.${namespace}.${status} is not empty`,
        );
      }
    }
  }
});
