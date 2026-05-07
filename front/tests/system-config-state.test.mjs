import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

import {
  SYSTEM_CONFIG_ALL_FILTER,
  buildSystemConfigHistoryDiffDisplay,
  buildSystemConfigDiffDisplay,
  buildSystemConfigOverrideDocumentText,
  countSystemConfigPersistenceIssues,
  canApplySystemConfigPreview,
  createDefaultSystemConfigFilters,
  filterSystemConfigFields,
  formatSystemConfigDocument,
  formatSystemConfigValue,
  sortSystemConfigPersistenceHealthItems,
  systemConfigPayloadsMatch,
} from "../src/pages/system-config/composables/systemConfigState.ts";

const ROOT = new URL("../", import.meta.url);

const baseField = {
  path: "log_level",
  section: "observability",
  value_kind: "string",
  editable: true,
  hot_reloadable: true,
  restart_required: false,
  sensitive: false,
  description: "Backend log level.",
  constraints: ["trace, debug, info, warn, or error"],
  value: "info",
  source: {
    kind: "default_file",
    source_name: "config.default.yaml",
    configured: true,
    warnings: [],
  },
};

test("filterSystemConfigFields filters by search, source, and boolean flags", () => {
  const fields = [
    baseField,
    {
      ...baseField,
      path: "secret_key",
      section: "security",
      value_kind: "string",
      editable: false,
      hot_reloadable: false,
      restart_required: true,
      sensitive: true,
      source: {
        ...baseField.source,
        kind: "environment",
        source_name: "environment",
      },
    },
  ];

  assert.deepEqual(
    filterSystemConfigFields(fields, {
      ...createDefaultSystemConfigFilters(),
      search: "secret",
      source: "environment",
      editable: "no",
      sensitive: "yes",
    }).map((field) => field.path),
    ["secret_key"],
  );

  assert.equal(
    filterSystemConfigFields(fields, {
      ...createDefaultSystemConfigFilters(),
      source: SYSTEM_CONFIG_ALL_FILTER,
    }).length,
    2,
  );
});

test("formatSystemConfigValue preserves redacted metadata without leaking raw secrets", () => {
  const display = formatSystemConfigValue({
    ...baseField,
    sensitive: true,
    value: {
      redacted: true,
      configured: true,
      length: 18,
      sha256_prefix: "abc123",
    },
  });

  assert.equal(display.redacted, true);
  assert.equal(display.configured, true);
  assert.equal(display.text, "");
  assert.equal(display.detail, "sha256:abc123 · 18 chars");
});

test("buildSystemConfigDiffDisplay converts diff values into stable text", () => {
  assert.deepEqual(
    buildSystemConfigDiffDisplay([
      {
        path: "provider_governance.enabled",
        old_value: true,
        new_value: false,
      },
      {
        path: "proxy_request",
        old_value: { total_timeout_seconds: 120 },
        new_value: { total_timeout_seconds: 60 },
      },
    ]),
    [
      {
        path: "provider_governance.enabled",
        oldText: "true",
        newText: "false",
      },
      {
        path: "proxy_request",
        oldText: '{\n  "total_timeout_seconds": 120\n}',
        newText: '{\n  "total_timeout_seconds": 60\n}',
      },
    ],
  );
});

test("history diff display summarizes redacted values without leaking hidden content", () => {
  const display = buildSystemConfigHistoryDiffDisplay([
    {
      path: "secret_key",
      old_value: {
        redacted: true,
        configured: true,
        length: 9,
        sha256_prefix: "old123",
      },
      new_value: {
        redacted: true,
        configured: true,
        length: 12,
        sha256_prefix: "new456",
      },
    },
  ]);

  assert.equal(display[0].oldText, "<redacted> sha256:old123 · 9 chars");
  assert.equal(display[0].newText, "<redacted> sha256:new456 · 12 chars");
});

test("formatSystemConfigDocument collapses nested redacted objects", () => {
  const text = formatSystemConfigDocument({
    secret_key: {
      redacted: true,
      configured: true,
      length: 11,
      sha256_prefix: "abc123",
    },
    log_level: "info",
  });

  assert.equal(
    text,
    '{\n  "secret_key": "<redacted> sha256:abc123 · 11 chars",\n  "log_level": "info"\n}',
  );
  assert.equal(text.includes("super-secret"), false);
});

test("override document text shows invalid path summary instead of empty YAML", () => {
  const text = buildSystemConfigOverrideDocumentText({
    path: "/tmp/config.override.yaml",
    exists: true,
    yaml: "",
    invalid_paths: ["db_url", "storage.s3.secret_key"],
    last_modified_ms: 1710000000000,
  });

  assert.match(text, /Override file is invalid/);
  assert.match(text, /db_url/);
  assert.match(text, /storage\.s3\.secret_key/);
  assert.notEqual(text, "{}");
});

test("persistence health helpers prioritize abnormal items", () => {
  const items = [
    {
      key: "local_storage_root",
      path: "/data/cyder/storage",
      exists: true,
      readable: true,
      writable: true,
      status: "ok",
      message: "ok",
    },
    {
      key: "sqlite_db_dir",
      path: "/data/cyder/db",
      exists: true,
      readable: true,
      writable: false,
      status: "error",
      message: "failed to write probe file",
    },
    {
      key: "s3_storage",
      path: "",
      exists: false,
      readable: false,
      writable: false,
      status: "skipped",
      message: "not required",
    },
    {
      key: "override_history",
      path: "/data/cyder/config/config.override.history.jsonl",
      exists: true,
      readable: true,
      writable: false,
      status: "warning",
      message: "append is degraded",
    },
  ];

  assert.deepEqual(
    sortSystemConfigPersistenceHealthItems(items).map((item) => item.key),
    ["sqlite_db_dir", "override_history", "local_storage_root", "s3_storage"],
  );
  assert.equal(countSystemConfigPersistenceIssues(items), 2);
});

const validPreview = {
  diff: [
    {
      path: "log_level",
      old_value: "info",
      new_value: "debug",
    },
  ],
  validation: {
    valid: true,
    errors: [],
    warnings: [],
  },
  next_override_yaml: "log_level: debug\n",
  runtime_actions: {
    update_runtime_snapshot: true,
    update_log_level: true,
    rebuild_http_client: false,
    hot_reloadable_paths: ["log_level"],
  },
  write_disabled_reason: null,
};

test("system config preview apply requires the current payload to match preview", () => {
  const previewPayload = {
    changes: {
      log_level: "debug",
    },
  };

  assert.equal(
    canApplySystemConfigPreview({
      preview: validPreview,
      previewPayload,
      currentPayload: {
        changes: {
          log_level: "debug",
        },
        reason: "reason changes are ignored",
      },
      reason: "enable debug logging",
      draftValidationError: null,
    }),
    true,
  );

  assert.equal(
    canApplySystemConfigPreview({
      preview: validPreview,
      previewPayload,
      currentPayload: {
        changes: {
          log_level: "trace",
        },
      },
      reason: "enable debug logging",
      draftValidationError: null,
    }),
    false,
  );
});

test("system config preview apply keeps no-diff preview disabled", () => {
  const payload = {
    changes: {
      log_level: "info",
    },
  };

  assert.equal(
    canApplySystemConfigPreview({
      preview: {
        ...validPreview,
        diff: [],
      },
      previewPayload: payload,
      currentPayload: payload,
      reason: "same value",
      draftValidationError: null,
    }),
    false,
  );
});

test("system config payload comparison is canonical and ignores reason", () => {
  assert.equal(
    systemConfigPayloadsMatch(
      {
        changes: {
          "proxy_request.total_timeout_seconds": null,
          "routing_resilience.max_candidates_per_request": 3,
        },
        reason: "preview reason",
      },
      {
        changes: {
          "routing_resilience.max_candidates_per_request": 3,
          "proxy_request.total_timeout_seconds": null,
        },
        reason: "apply reason",
      },
    ),
    true,
  );
});

test("system config page uses page-local entry point and composable state", async () => {
  const routerSource = await readFile(new URL("src/router/index.ts", ROOT), "utf8");

  assert.match(routerSource, /pages\/system-config\/SystemConfigPage\.vue/);
  assert.equal(routerSource.includes("@/pages/SystemConfig.vue"), false);
  assert.equal(routerSource.includes("@/pages/systemConfigState"), false);

  await assert.rejects(() => access(new URL("src/pages/SystemConfig.vue", ROOT)));
  await assert.rejects(() => access(new URL("src/pages/systemConfigState.ts", ROOT)));
});
