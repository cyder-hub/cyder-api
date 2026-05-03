import test from "node:test";
import assert from "node:assert/strict";

import { enDict } from "../src/i18n/en.ts";
import { zhDict } from "../src/i18n/zh.ts";

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

test("alert and notification pages translate every delivery status", () => {
  for (const [locale, dict] of [
    ["en", enDict],
    ["zh", zhDict],
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
