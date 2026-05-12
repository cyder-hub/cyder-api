import test from "node:test";
import assert from "node:assert/strict";

import { copyText } from "../src/utils/clipboard.ts";
import {
  getStoredAppLocale,
  LANG_STORAGE_KEY,
  resolveIntlLocale,
  setStoredAppLocale,
} from "../src/i18n/locale.ts";
import { formatTimestamp } from "../src/utils/datetime.ts";
import { normalizeError } from "../src/utils/error.ts";
import {
  formatCostRateFromNanos,
  formatCostRateInputFromNanos,
  formatPriceFromNanos,
  formatPriceInputFromNanos,
  majorUnitToNanos,
  nanosToMajorUnit,
  parseCostRateInputToNanos,
} from "../src/utils/money.ts";
import { formatNumberValue } from "../src/utils/number.ts";

function withMockLocalStorage(callback) {
  const originalDescriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    "localStorage",
  );
  const store = new Map();
  const localStorage = {
    getItem: (key) => (store.has(key) ? store.get(key) : null),
    setItem: (key, value) => store.set(key, String(value)),
    removeItem: (key) => store.delete(key),
    clear: () => store.clear(),
  };

  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: localStorage,
  });

  try {
    callback(localStorage);
  } finally {
    if (originalDescriptor) {
      Object.defineProperty(globalThis, "localStorage", originalDescriptor);
    } else {
      delete globalThis.localStorage;
    }
  }
}

test("formatTimestamp uses the requested locale and rejects empty values", () => {
  assert.equal(formatTimestamp(null), "");
  assert.equal(formatTimestamp(0), "");
  assert.match(formatTimestamp(Date.UTC(2026, 4, 7, 8, 9, 10), "en"), /2026|05|07/);
  assert.match(formatTimestamp(Date.UTC(2026, 4, 7, 8, 9, 10), "zh"), /2026|05|07/);
});

test("locale helpers own storage and Intl locale resolution", () => {
  withMockLocalStorage((localStorage) => {
    assert.equal(getStoredAppLocale(), "en");
    assert.equal(resolveIntlLocale(), "en-US");

    setStoredAppLocale("zh");
    assert.equal(localStorage.getItem(LANG_STORAGE_KEY), "zh");
    assert.equal(getStoredAppLocale(), "zh");
    assert.equal(resolveIntlLocale(), "zh-CN");

    localStorage.setItem(LANG_STORAGE_KEY, "unsupported");
    assert.equal(getStoredAppLocale(), "en");
    assert.equal(resolveIntlLocale("fr-FR"), "fr-FR");
  });
});

test("formatNumberValue uses explicit locale input", () => {
  const options = { minimumFractionDigits: 1, maximumFractionDigits: 1 };
  assert.equal(
    formatNumberValue(1234.5, options, "de-DE"),
    new Intl.NumberFormat("de-DE", options).format(1234.5),
  );
});

test("money helpers convert major units, nanos, and per-million rates", () => {
  assert.equal(majorUnitToNanos("1.25", "USD"), 1_250_000_00000);
  assert.equal(nanosToMajorUnit(1_250_000_00000, "USD"), 1.25);
  assert.equal(formatPriceInputFromNanos(1_250_000_00000, "USD"), "1.25");
  assert.equal(formatPriceFromNanos(1_250_000_00000, "USD"), "USD 1.25");

  assert.equal(parseCostRateInputToNanos("0.12", "per_million_units", "USD"), 12000);
  assert.equal(formatCostRateInputFromNanos(12000, "per_million_units", "USD"), "0.12");
  assert.equal(formatCostRateFromNanos(12000, "per_million_units", "USD"), "USD 0.12 / 1M");
  assert.equal(
    formatPriceFromNanos(1_250_000_00000, "USD", "-", "de-DE"),
    `USD ${new Intl.NumberFormat("de-DE", {
      minimumFractionDigits: 0,
      maximumFractionDigits: 11,
    }).format(1.25)}`,
  );
});

test("normalizeError preserves useful messages and falls back for unknown values", () => {
  const existing = new Error("already normalized");
  assert.equal(normalizeError(existing), existing);
  assert.equal(normalizeError("failed").message, "failed");
  assert.equal(normalizeError({ reason: "hidden" }, "fallback").message, "fallback");
});

test("copyText returns false outside browser clipboard environments", async () => {
  assert.equal(await copyText("secret"), false);
  assert.equal(await copyText(""), false);
});
