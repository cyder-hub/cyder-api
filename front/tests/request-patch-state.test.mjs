import test from "node:test";
import assert from "node:assert/strict";

import {
  buildRequestPatchRuleInput,
  buildRequestPatchVariantPayload,
  formatRequestPatchValueForDisplay,
  formatRequestPatchValueForEditor,
} from "../src/utils/requestPatch.ts";

test("request patch rule editor parses and normalizes JSON values", () => {
  assert.deepEqual(
    buildRequestPatchRuleInput({
      placement: "BODY",
      target: " /generationConfig ",
      operation: "SET",
      value_json_text: '{ "temperature": 0.2, "enabled": true }',
      description: " provider default ",
    }),
    {
      error: null,
      input: {
        placement: "BODY",
        target: "/generationConfig",
        operation: "SET",
        value_json: { temperature: 0.2, enabled: true },
        description: "provider default",
      },
    },
  );

  assert.equal(
    buildRequestPatchRuleInput({
      placement: "HEADER",
      target: "authorization",
      operation: "REMOVE",
      value_json_text: "ignored",
      description: "",
    }).input?.value_json,
    null,
  );
});

test("whole Variant payload includes source identity, metadata, and all Rules", () => {
  const result = buildRequestPatchVariantPayload({
    source_id: 101,
    model_id: 42,
    suffix: "reasoning",
    enabled: true,
    expose_in_models: true,
    rules: [
      {
        placement: "QUERY",
        target: "api-version",
        operation: "SET",
        value_json_text: "2026",
        description: "version",
      },
      {
        placement: "BODY",
        target: "/metadata/tag",
        operation: "SET",
        value_json_text: "null",
        description: "",
      },
    ],
  });

  assert.equal(result.error, null);
  assert.deepEqual(result.payload, {
    source_id: 101,
    model_id: 42,
    suffix: "reasoning",
    enabled: true,
    expose_in_models: true,
    rules: [
      {
        placement: "QUERY",
        target: "api-version",
        operation: "SET",
        value_json: 2026,
        description: "version",
      },
      {
        placement: "BODY",
        target: "/metadata/tag",
        operation: "SET",
        value_json: null,
        description: null,
      },
    ],
  });
});

test("request patch editor rejects missing and invalid SET values", () => {
  assert.equal(
    buildRequestPatchVariantPayload({
      source_id: 101,
      model_id: null,
      suffix: null,
      enabled: true,
      expose_in_models: false,
      rules: [{
        placement: "HEADER",
        target: "x-test",
        operation: "SET",
        value_json_text: "not json",
        description: "",
      }],
    }).error,
    "invalid",
  );
});

test("stored JSON values remain editable and readable", () => {
  assert.equal(
    formatRequestPatchValueForEditor('{"temperature":0.2}'),
    '{\n  "temperature": 0.2\n}',
  );
  assert.equal(formatRequestPatchValueForEditor('"Bearer token"'), '"Bearer token"');
  assert.equal(formatRequestPatchValueForDisplay('{"temperature":0.2}'), '{"temperature":0.2}');
  assert.equal(formatRequestPatchValueForDisplay('"Bearer token"'), '"Bearer token"');
});
