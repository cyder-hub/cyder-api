import test from "node:test";
import assert from "node:assert/strict";

import {
  buildPortableDownloadFilename,
  canApplyPortableImport,
  createDefaultPortableExportSelections,
  createDefaultPortableImportSelections,
  getPortableApplyDisabledReasonCode,
  getPortableExportDisabledReason,
  hasPortableBlockingState,
  mergeDangerousPatchConfirmations,
  updateDangerousPatchConfirmation,
} from "../src/pages/export-import/composables/portableConfigState.ts";

function summary(overrides = {}) {
  return {
    total: 0,
    create: 0,
    update: 0,
    skip: 0,
    blocked: 0,
    conflict: 0,
    ...overrides,
  };
}

function subrange(subrangeId, overrides = {}) {
  return {
    subrange_id: subrangeId,
    label: subrangeId,
    default_selected: true,
    required: false,
    contains_secrets: false,
    deferred: false,
    deferred_reason: null,
    ...overrides,
  };
}

function module(moduleId, overrides = {}) {
  return {
    module_id: moduleId,
    label: moduleId,
    description: "",
    module_version: 1,
    default_selected: false,
    contains_secrets: false,
    deferred: false,
    deferred_reason: null,
    dependencies: [],
    subranges: [],
    conflict_strategies: [
      "fail_on_conflict",
      "skip_existing",
      "overwrite_existing",
    ],
    ...overrides,
  };
}

function registry(overrides = {}) {
  return {
    schema_version: "cyder.portable.v1",
    modules: [
      module("provider_profile", {
        default_selected: true,
        contains_secrets: true,
        subranges: [
          subrange("provider_core", { required: true }),
          subrange("provider_keys", { required: true, contains_secrets: true }),
          subrange("provider_models"),
          subrange("provider_request_patches", { default_selected: false }),
        ],
      }),
      module("api_keys", {
        default_selected: true,
        contains_secrets: true,
        dependencies: [
          {
            module_id: "provider_profile",
            required_for_export: false,
            required_for_fresh_import: true,
            reason: "needs provider refs",
          },
        ],
        subranges: [
          subrange("api_key_core", { required: true, contains_secrets: true }),
          subrange("api_key_acl"),
          subrange("api_key_model_override"),
        ],
      }),
      module("cost_catalogs", {
        default_selected: false,
        deferred: overrides.costDeferred ?? false,
        deferred_reason: overrides.costDeferred ? "later" : null,
        subranges: [subrange("cost_catalog_core", { required: true })],
      }),
      module("cost_bindings", {
        default_selected: false,
        deferred: overrides.bindingDeferred ?? false,
        deferred_reason: overrides.bindingDeferred ? "later" : null,
        dependencies: [
          {
            module_id: "provider_profile",
            required_for_export: true,
            required_for_fresh_import: true,
            reason: "needs provider profile",
          },
          {
            module_id: "cost_catalogs",
            required_for_export: true,
            required_for_fresh_import: true,
            reason: "needs cost catalogs",
          },
        ],
        subranges: [subrange("cost_model_bindings", { required: true })],
      }),
    ],
    default_selected_modules: ["provider_profile", "api_keys"],
    apply_order: [
      "cost_catalogs",
      "provider_profile",
      "cost_bindings",
      "api_keys",
    ],
  };
}

function preview(overrides = {}) {
  return {
    schema_version: "cyder.portable.v1",
    exported_at: 1778236800000,
    cyder_version: "1.0.0",
    bundle_digest: "sha256:test",
    file_protection: {
      mode: "plaintext",
      requires_password: false,
      decrypted: true,
      integrity_checked: false,
      integrity_valid: null,
    },
    modules: [
      {
        module_id: "provider_profile",
        module_version: 1,
        label: "Provider Profile",
        supported: true,
        available: true,
        selected_by_default: true,
        contains_secrets: true,
        deferred: false,
        dependencies: [],
        subranges: ["provider_core", "provider_keys", "provider_models"],
        summary: summary({ total: 2, create: 2 }),
        warnings: [],
        blocking_issues: [],
      },
      {
        module_id: "api_keys",
        module_version: 1,
        label: "API Keys",
        supported: true,
        available: true,
        selected_by_default: true,
        contains_secrets: true,
        deferred: false,
        dependencies: [],
        subranges: ["api_key_core", "api_key_acl"],
        summary: summary({ total: 1, create: 1 }),
        warnings: [],
        blocking_issues: [],
      },
    ],
    default_selected_modules: ["provider_profile", "api_keys"],
    unsupported_modules: [],
    blocking_issues: [],
    excluded_data_types: [],
    ...overrides,
  };
}

test("portable export defaults select core modules and required/default subranges", () => {
  const selections = createDefaultPortableExportSelections(
    registry({ costDeferred: true, bindingDeferred: true }),
  );

  assert.deepEqual(
    selections.map((selection) => selection.module_id),
    ["provider_profile", "api_keys"],
  );
  assert.deepEqual(selections[0].subranges, [
    "provider_core",
    "provider_keys",
    "provider_models",
  ]);
  assert.deepEqual(selections[1].subranges, [
    "api_key_core",
    "api_key_acl",
    "api_key_model_override",
  ]);
});

test("cost bindings stay disabled until provider profile and cost catalogs are selected", () => {
  const portableRegistry = registry();
  const bindingModule = portableRegistry.modules.find(
    (item) => item.module_id === "cost_bindings",
  );

  assert.equal(
    getPortableExportDisabledReason(bindingModule, new Set(["provider_profile"])),
    "needs cost catalogs",
  );
  assert.equal(
    getPortableExportDisabledReason(
      bindingModule,
      new Set(["provider_profile", "cost_catalogs"]),
    ),
    null,
  );
});

test("portable import preview selects available defaults and blocks apply on top-level issues", () => {
  const blockedPreview = preview({
    blocking_issues: [
      {
        code: "password_required",
        message: "password required",
        path: "$",
        target: null,
        module_id: null,
        subrange_id: null,
      },
    ],
  });

  assert.deepEqual(createDefaultPortableImportSelections(blockedPreview), []);
  assert.equal(hasPortableBlockingState(blockedPreview), true);
  assert.equal(
    getPortableApplyDisabledReasonCode({
      preview: blockedPreview,
      selectedModules: [{ module_id: "provider_profile", subranges: [] }],
      conflictStrategy: "skip_existing",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    "top_level_blocking",
  );
  assert.equal(
    canApplyPortableImport({
      preview: blockedPreview,
      selectedModules: [{ module_id: "provider_profile", subranges: [] }],
      conflictStrategy: "skip_existing",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    false,
  );
});

test("portable import apply gates selected modules on non-ignorable blocking issues", () => {
  const conflictIssue = {
    code: "conflict",
    message: "target exists",
    path: "$.modules[1].items[0]",
    target: null,
    module_id: "api_keys",
    subrange_id: "api_key_core",
  };
  const conflictPreview = preview({
    modules: [
      preview().modules[0],
      {
        ...preview().modules[1],
        summary: summary({ total: 1, conflict: 1 }),
        blocking_issues: [conflictIssue],
      },
    ],
  });
  const selectedModules = createDefaultPortableImportSelections(conflictPreview);

  assert.equal(
    getPortableApplyDisabledReasonCode({
      preview: conflictPreview,
      selectedModules,
      conflictStrategy: "fail_on_conflict",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    "module_blocking",
  );
  assert.equal(
    canApplyPortableImport({
      preview: conflictPreview,
      selectedModules,
      conflictStrategy: "fail_on_conflict",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    false,
  );
  assert.equal(
    canApplyPortableImport({
      preview: conflictPreview,
      selectedModules,
      conflictStrategy: "skip_existing",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    true,
  );

  const aclMissingDependencyPreview = preview({
    modules: [
      preview().modules[0],
      {
        ...preview().modules[1],
        summary: summary({ total: 1, blocked: 1 }),
        blocking_issues: [
          {
            code: "missing_dependency",
            message: "provider missing",
            path: "$.modules[1].items[0].acl_rules[0]",
            target: null,
            module_id: "api_keys",
            subrange_id: "api_key_acl",
          },
        ],
      },
    ],
  });
  assert.equal(
    getPortableApplyDisabledReasonCode({
      preview: aclMissingDependencyPreview,
      selectedModules: createDefaultPortableImportSelections(
        aclMissingDependencyPreview,
      ),
      conflictStrategy: "overwrite_existing",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    "module_blocking",
  );
});

test("portable import apply allows missing route override blocks to be skipped by backend", () => {
  const routeOverridePreview = preview({
    modules: [
      preview().modules[0],
      {
        ...preview().modules[1],
        summary: summary({ total: 2, create: 1, blocked: 1 }),
        blocking_issues: [
          {
            code: "missing_dependency",
            message: "route missing",
            path: "$.modules[1].items[0].model_overrides[0]",
            target: null,
            module_id: "api_keys",
            subrange_id: "api_key_model_override",
          },
        ],
      },
    ],
  });

  assert.equal(hasPortableBlockingState(routeOverridePreview), true);
  assert.equal(
    getPortableApplyDisabledReasonCode({
      preview: routeOverridePreview,
      selectedModules: createDefaultPortableImportSelections(routeOverridePreview),
      conflictStrategy: "fail_on_conflict",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    null,
  );
  assert.equal(
    canApplyPortableImport({
      preview: routeOverridePreview,
      selectedModules: createDefaultPortableImportSelections(routeOverridePreview),
      conflictStrategy: "fail_on_conflict",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    true,
  );
});

test("portable import apply requires a selected module and operator reason", () => {
  const cleanPreview = preview();
  const selectedModules = createDefaultPortableImportSelections(cleanPreview);

  assert.equal(
    canApplyPortableImport({
      preview: cleanPreview,
      selectedModules: [],
      conflictStrategy: "fail_on_conflict",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    false,
  );
  assert.equal(
    canApplyPortableImport({
      preview: cleanPreview,
      selectedModules,
      conflictStrategy: "fail_on_conflict",
      reason: "   ",
      dangerousPatchConfirmations: [],
    }),
    false,
  );
  assert.equal(
    canApplyPortableImport({
      preview: cleanPreview,
      selectedModules,
      conflictStrategy: "fail_on_conflict",
      reason: "restore config",
      dangerousPatchConfirmations: [],
    }),
    true,
  );
});

test("dangerous patch confirmation gates apply while conflict strategy remains selectable", () => {
  const dangerousPreview = preview({
    modules: [
      {
        ...preview().modules[0],
        blocking_issues: [
          {
            code: "dangerous_request_patch_confirmation_required",
            message: "confirm dangerous target",
            path: "modules[0].items.request_patches[0]",
            target: "/messages/0/content",
            module_id: "provider_profile",
            subrange_id: "provider_request_patches",
          },
        ],
      },
    ],
  });
  const selectedModules = createDefaultPortableImportSelections(dangerousPreview);
  const confirmations = mergeDangerousPatchConfirmations(dangerousPreview, []);

  assert.equal(confirmations.length, 1);
  assert.equal(
    getPortableApplyDisabledReasonCode({
      preview: dangerousPreview,
      selectedModules,
      conflictStrategy: "overwrite_existing",
      reason: "restore config",
      dangerousPatchConfirmations: confirmations,
    }),
    "dangerous_patch_confirmation",
  );
  assert.equal(
    canApplyPortableImport({
      preview: dangerousPreview,
      selectedModules,
      conflictStrategy: "overwrite_existing",
      reason: "restore config",
      dangerousPatchConfirmations: confirmations,
    }),
    false,
  );

  const confirmed = updateDangerousPatchConfirmation(
    confirmations,
    "modules[0].items.request_patches[0]",
    "/messages/0/content",
    true,
  );
  assert.equal(
    getPortableApplyDisabledReasonCode({
      preview: dangerousPreview,
      selectedModules,
      conflictStrategy: "overwrite_existing",
      reason: "restore config",
      dangerousPatchConfirmations: confirmed,
    }),
    null,
  );
  assert.equal(
    canApplyPortableImport({
      preview: dangerousPreview,
      selectedModules,
      conflictStrategy: "overwrite_existing",
      reason: "restore config",
      dangerousPatchConfirmations: confirmed,
    }),
    true,
  );
});

test("portable download filename uses cyd extension and strips unsafe path characters", () => {
  assert.equal(buildPortableDownloadFilename(null), "cyder-portable-config.cyd");
  assert.equal(
    buildPortableDownloadFilename("../backup:prod"),
    "backup-prod.cyd",
  );
  assert.equal(
    buildPortableDownloadFilename("cyder-export.cyd"),
    "cyder-export.cyd",
  );
});
