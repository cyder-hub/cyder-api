import test from "node:test";
import assert from "node:assert/strict";

import { copyText } from "../src/utils/clipboard.ts";
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

test("formatTimestamp uses the requested locale and rejects empty values", () => {
  assert.equal(formatTimestamp(null), "");
  assert.equal(formatTimestamp(0), "");
  assert.match(formatTimestamp(Date.UTC(2026, 4, 7, 8, 9, 10), "en"), /2026|05|07/);
  assert.match(formatTimestamp(Date.UTC(2026, 4, 7, 8, 9, 10), "zh"), /2026|05|07/);
});

test("money helpers convert major units, nanos, and per-million rates", () => {
  assert.equal(majorUnitToNanos("1.25", "USD"), 1_250_000_00000);
  assert.equal(nanosToMajorUnit(1_250_000_00000, "USD"), 1.25);
  assert.equal(formatPriceInputFromNanos(1_250_000_00000, "USD"), "1.25");
  assert.equal(formatPriceFromNanos(1_250_000_00000, "USD"), "USD 1.25");

  assert.equal(parseCostRateInputToNanos("0.12", "per_million_units", "USD"), 12000);
  assert.equal(formatCostRateInputFromNanos(12000, "per_million_units", "USD"), "0.12");
  assert.equal(formatCostRateFromNanos(12000, "per_million_units", "USD"), "USD 0.12 / 1M");
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
