import test from "node:test";
import assert from "node:assert/strict";

import {
  calculateRecordTps,
  resolveVisibleOutputTokens,
} from "../src/pages/record/composables/useRecordList.ts";

test("record TPS uses streaming tail duration when the stream has a meaningful tail", () => {
  const result = calculateRecordTps({
    is_stream: true,
    total_output_tokens: 60,
    output_text_tokens: 50,
    reasoning_tokens: 10,
    first_attempt_started_at: 1_000,
    response_started_to_client_at: 3_000,
    completed_at: 8_000,
  });

  assert.equal(result?.durationKind, "stream_tail");
  assert.equal(result?.durationMs, 5_000);
  assert.equal(result?.tokens, 50);
  assert.equal(result?.value, 10);
});

test("record TPS falls back to effective duration for single-chunk-like streams", () => {
  const result = calculateRecordTps({
    is_stream: true,
    output_text_tokens: 50,
    first_attempt_started_at: 1_000,
    response_started_to_client_at: 9_700,
    completed_at: 10_000,
  });

  assert.equal(result?.durationKind, "effective");
  assert.equal(result?.durationMs, 9_000);
  assert.equal(Number(result?.value.toFixed(2)), 5.56);
});

test("record TPS uses visible tokens and effective duration for non-stream records", () => {
  const result = calculateRecordTps({
    is_stream: false,
    total_output_tokens: 60,
    reasoning_tokens: 20,
    first_attempt_started_at: 1_000,
    response_started_to_client_at: 1_500,
    completed_at: 5_000,
  });

  assert.equal(result?.durationKind, "effective");
  assert.equal(result?.tokens, 40);
  assert.equal(result?.value, 10);
});

test("visible token resolution prefers normalized output text tokens", () => {
  assert.equal(
    resolveVisibleOutputTokens({
      output_text_tokens: 18,
      total_output_tokens: 25,
      reasoning_tokens: 7,
    }),
    18,
  );
  assert.equal(
    resolveVisibleOutputTokens({
      total_output_tokens: 25,
      reasoning_tokens: 7,
    }),
    18,
  );
  assert.equal(resolveVisibleOutputTokens({ total_output_tokens: 25 }), 25);
});

test("record TPS returns null when usable tokens or timestamps are missing", () => {
  assert.equal(calculateRecordTps({ output_text_tokens: 0 }), null);
  assert.equal(
    calculateRecordTps({
      output_text_tokens: 10,
      first_attempt_started_at: 5_000,
      completed_at: 5_000,
    }),
    null,
  );
});
