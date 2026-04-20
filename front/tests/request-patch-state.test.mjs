import test from "node:test";
import assert from "node:assert/strict";

import {
  buildRequestPatchPayloadFromEditor,
  formatRequestPatchValueForDisplay,
  formatRequestPatchValueForEditor,
} from "../src/lib/requestPatch.ts";

test("request patch editor payload preserves JSON object, scalar, and null values", () => {
  assert.deepEqual(
    buildRequestPatchPayloadFromEditor({
      placement: "BODY",
      target: " /generationConfig ",
      operation: "SET",
      value_json_text: '{ "temperature": 0.2, "enabled": true }',
      description: " provider default ",
      is_enabled: true,
    }),
    {
      placement: "BODY",
      target: "/generationConfig",
      operation: "SET",
      value_json: { temperature: 0.2, enabled: true },
      description: "provider default",
      is_enabled: true,
      confirm_dangerous_target: false,
    },
  );

  assert.deepEqual(
    buildRequestPatchPayloadFromEditor({
      placement: "QUERY",
      target: "api-version",
      operation: "SET",
      value_json_text: "2026",
      description: "",
      is_enabled: false,
    }),
    {
      placement: "QUERY",
      target: "api-version",
      operation: "SET",
      value_json: 2026,
      description: null,
      is_enabled: false,
      confirm_dangerous_target: false,
    },
  );

  assert.deepEqual(
    buildRequestPatchPayloadFromEditor({
      placement: "BODY",
      target: "/metadata/tag",
      operation: "SET",
      value_json_text: "null",
      description: "",
      is_enabled: true,
    }),
    {
      placement: "BODY",
      target: "/metadata/tag",
      operation: "SET",
      value_json: null,
      description: null,
      is_enabled: true,
      confirm_dangerous_target: false,
    },
  );
});

test("request patch editor payload omits value_json for REMOVE and carries dangerous confirmation", () => {
  const payload = buildRequestPatchPayloadFromEditor(
    {
      placement: "HEADER",
      target: " authorization ",
      operation: "REMOVE",
      value_json_text: '"ignored"',
      description: " clear upstream auth ",
      is_enabled: true,
    },
    true,
  );

  assert.deepEqual(payload, {
    placement: "HEADER",
    target: "authorization",
    operation: "REMOVE",
    description: "clear upstream auth",
    is_enabled: true,
    confirm_dangerous_target: true,
  });
  assert.equal(Object.hasOwn(payload, "value_json"), false);
});

test("request patch value formatting keeps parsed detail values editable without stringifying their semantics", () => {
  assert.equal(
    formatRequestPatchValueForEditor({ temperature: 0.2 }),
    '{\n  "temperature": 0.2\n}',
  );
  assert.equal(formatRequestPatchValueForEditor("Bearer token"), '"Bearer token"');
  assert.equal(formatRequestPatchValueForDisplay({ temperature: 0.2 }), '{"temperature":0.2}');
  assert.equal(formatRequestPatchValueForDisplay("Bearer token"), "Bearer token");
});
