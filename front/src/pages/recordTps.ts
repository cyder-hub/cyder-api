export type RecordTpsDurationKind = "stream_tail" | "effective";

export interface RecordTpsInput {
  total_output_tokens?: number | null;
  output_text_tokens?: number | null;
  reasoning_tokens?: number | null;
  first_attempt_started_at?: number | null;
  response_started_to_client_at?: number | null;
  completed_at?: number | null;
  is_stream?: boolean | null;
}

export interface RecordTpsCalculation {
  value: number;
  tokens: number;
  durationMs: number;
  durationKind: RecordTpsDurationKind;
}

export const STREAM_TPS_MIN_TAIL_MS = 750;
export const STREAM_TPS_MIN_TAIL_RATIO = 0.05;
export const STREAM_TPS_MIN_OUTPUT_TOKENS = 8;

const finiteNonNegativeNumber = (value: number | null | undefined) =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;

const finiteTimestamp = (value: number | null | undefined) =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

export const resolveVisibleOutputTokens = (record: RecordTpsInput) => {
  const outputTextTokens = finiteNonNegativeNumber(record.output_text_tokens);
  if (outputTextTokens != null) {
    return outputTextTokens;
  }

  const totalOutputTokens = finiteNonNegativeNumber(record.total_output_tokens);
  if (totalOutputTokens == null) {
    return null;
  }

  const reasoningTokens = finiteNonNegativeNumber(record.reasoning_tokens);
  if (reasoningTokens != null) {
    return Math.max(totalOutputTokens - reasoningTokens, 0);
  }

  return totalOutputTokens;
};

export const calculateRecordTps = (
  record: RecordTpsInput,
): RecordTpsCalculation | null => {
  const tokens = resolveVisibleOutputTokens(record);
  const startedAt = finiteTimestamp(record.first_attempt_started_at);
  const completedAt = finiteTimestamp(record.completed_at);

  if (tokens == null || tokens <= 0 || startedAt == null || completedAt == null) {
    return null;
  }

  const totalMs = completedAt - startedAt;
  if (totalMs <= 0) {
    return null;
  }

  let durationMs = totalMs;
  let durationKind: RecordTpsDurationKind = "effective";
  const firstTokenAt = finiteTimestamp(record.response_started_to_client_at);
  const streamTailMs = firstTokenAt == null ? null : completedAt - firstTokenAt;
  const canUseStreamTail =
    record.is_stream === true &&
    streamTailMs != null &&
    streamTailMs > 0 &&
    streamTailMs >= STREAM_TPS_MIN_TAIL_MS &&
    streamTailMs / totalMs >= STREAM_TPS_MIN_TAIL_RATIO &&
    tokens >= STREAM_TPS_MIN_OUTPUT_TOKENS;

  if (canUseStreamTail && streamTailMs != null) {
    durationMs = streamTailMs;
    durationKind = "stream_tail";
  }

  return {
    value: tokens / (durationMs / 1000),
    tokens,
    durationMs,
    durationKind,
  };
};
