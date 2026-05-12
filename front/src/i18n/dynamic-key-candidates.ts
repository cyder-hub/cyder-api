export interface DynamicI18nKeySource {
  id: string;
  keyTemplates: readonly string[];
  values: readonly string[];
  placeholders?: Readonly<Record<string, readonly string[]>>;
  valueSource: string;
  notes?: string;
}

export interface DynamicI18nFallbackException {
  id: string;
  keyTemplate: string;
  guard: string;
  fallback: string;
}

export const DYNAMIC_I18N_KEY_SOURCES = [
  {
    id: "route-title",
    keyTemplates: ["{route.meta.titleKey}"],
    values: [],
    valueSource: "front/src/router/index.ts route meta.titleKey",
  },
  {
    id: "sidebar-item",
    keyTemplates: ["{navItem.i18nKey}"],
    values: [],
    valueSource: "front/src/router/nav-items.ts navItems[].i18nKey",
  },
  {
    id: "sidebar-section",
    keyTemplates: ["sidebar.sections.{section}"],
    values: ["operations", "traffic", "resources", "governance"],
    placeholders: {
      section: ["operations", "traffic", "resources", "governance"],
    },
    valueSource: "front/src/router/nav-items.ts NavSection",
  },
  {
    id: "dashboard-usage-metric",
    keyTemplates: ["dashboard.usageStats.metrics.{metric}"],
    values: [
      "total_tokens",
      "request_count",
      "total_cost",
      "success_rate",
      "error_count",
      "avg_latency",
      "total_input_tokens",
      "total_output_tokens",
      "total_reasoning_tokens",
    ],
    placeholders: {
      metric: [
        "total_tokens",
        "request_count",
        "total_cost",
        "success_rate",
        "error_count",
        "avg_latency",
        "total_input_tokens",
        "total_output_tokens",
        "total_reasoning_tokens",
      ],
    },
    valueSource: "front/src/components/UsageChart.vue metricOptions",
  },
  {
    id: "request-patch-prefix",
    keyTemplates: [
      "{textPrefix}.placements.{placement}",
      "{textPrefix}.operations.{operation}",
    ],
    values: [
      "providerEditPage.requestPatch",
      "modelEditPage.requestPatch",
      "HEADER",
      "QUERY",
      "BODY",
      "SET",
      "REMOVE",
    ],
    placeholders: {
      textPrefix: ["providerEditPage.requestPatch", "modelEditPage.requestPatch"],
      placement: ["HEADER", "QUERY", "BODY"],
      operation: ["SET", "REMOVE"],
    },
    valueSource:
      "front/src/components/request-patch/RequestPatchRulesPanel.vue props.textPrefix",
  },
  {
    id: "alerts",
    keyTemplates: [
      "alertsPage.severity.{severity}",
      "alertsPage.status.{status}",
      "alertsPage.delivery.status.{deliveryStatus}",
    ],
    values: [
      "critical",
      "warning",
      "info",
      "active",
      "resolved",
      "failed",
      "retry_scheduled",
      "skipped",
      "in_progress",
      "pending",
      "succeeded",
    ],
    placeholders: {
      severity: ["critical", "warning", "info"],
      status: ["active", "resolved"],
      deliveryStatus: [
        "failed",
        "retry_scheduled",
        "skipped",
        "in_progress",
        "pending",
        "succeeded",
      ],
    },
    valueSource: "front/src/pages/alerts/** alert and delivery state values",
  },
  {
    id: "notifications",
    keyTemplates: ["notificationPage.delivery.status.{deliveryStatus}"],
    values: [
      "failed",
      "retry_scheduled",
      "skipped",
      "in_progress",
      "pending",
      "succeeded",
    ],
    placeholders: {
      deliveryStatus: [
        "failed",
        "retry_scheduled",
        "skipped",
        "in_progress",
        "pending",
        "succeeded",
      ],
    },
    valueSource: "front/src/pages/notifications/** delivery state values",
  },
  {
    id: "api-key-governance",
    keyTemplates: [
      "apiKeyPage.status.{lifecycle}",
      "apiKeyPage.runtimeRejection.{reason}",
    ],
    values: [
      "active",
      "disabled",
      "expired",
      "expiringSoon",
      "none",
      "rpm",
      "concurrency",
      "dailyRequests",
      "dailyTokens",
      "monthlyTokens",
      "dailyBudget",
      "monthlyBudget",
    ],
    placeholders: {
      lifecycle: ["active", "disabled", "expired", "expiringSoon"],
      reason: [
        "none",
        "disabled",
        "expired",
        "rpm",
        "concurrency",
        "dailyRequests",
        "dailyTokens",
        "monthlyTokens",
        "dailyBudget",
        "monthlyBudget",
      ],
    },
    valueSource: "front/src/pages/api-key/** lifecycle and runtime rejection values",
  },
  {
    id: "api-key-edit-modal",
    keyTemplates: [
      "apiKeyEditModal.action.{action}",
      "apiKeyEditModal.scope.{scope}",
      "apiKeyEditModal.currency.{currency}",
    ],
    values: ["ALLOW", "DENY", "PROVIDER", "MODEL", "USD", "CNY"],
    placeholders: {
      action: ["ALLOW", "DENY"],
      scope: ["PROVIDER", "MODEL"],
      currency: ["USD", "CNY"],
    },
    valueSource: "front/src/pages/api-key/components/ApiKeyEditDialog.vue",
  },
  {
    id: "model-capabilities",
    keyTemplates: ["{capability.labelKey}"],
    values: [],
    valueSource:
      "MODEL_CAPABILITY_ITEMS and provider edit capability item labelKey values",
  },
  {
    id: "cost-options",
    keyTemplates: ["{option.labelKey}"],
    values: [],
    valueSource:
      "front/src/pages/cost/helpers.ts METER_OPTIONS, CHARGE_KIND_OPTIONS, TIER_BASIS_OPTIONS",
  },
  {
    id: "cost-version-state",
    keyTemplates: [
      "costPage.state.{state}",
      "costPage.versionDetail.{state}Description",
    ],
    values: ["archived", "active", "frozen", "draft"],
    placeholders: {
      state: ["archived", "active", "frozen", "draft"],
    },
    valueSource:
      "front/src/pages/cost/** versionStateLabel and readOnlyStateDescription",
  },
  {
    id: "cost-validation-alert",
    keyTemplates: ["costPage.alert.{messagePath}"],
    values: [
      "tier.multiple_unbounded",
      "tier.not_increasing",
      "tier.unbounded_not_last",
    ],
    placeholders: {
      messagePath: [
        "tier.multiple_unbounded",
        "tier.not_increasing",
        "tier.unbounded_not_last",
      ],
    },
    valueSource: "front/src/pages/cost/** validation message path mapping",
  },
  {
    id: "portable-config-enums",
    keyTemplates: [
      "portableConfigPage.fileProtection.{mode}",
      "portableConfigPage.conflictStrategy.{strategy}",
      "portableConfigPage.applyStatus.{status}",
      "portableConfigPage.import.applyDisabledReason.{code}",
    ],
    values: [],
    placeholders: {
      mode: ["plaintext", "password_encrypted"],
      strategy: ["fail_on_conflict", "skip_existing", "overwrite_existing"],
      status: ["applied", "skipped", "blocked", "failed"],
      code: [
        "no_preview",
        "top_level_blocking",
        "no_selected_modules",
        "missing_reason",
        "module_blocking",
        "dangerous_patch_confirmation",
      ],
    },
    valueSource:
      "front/src/services/types/portableConfig.ts and PortableApplyDisabledReasonCode",
  },
  {
    id: "portable-config-known-ids",
    keyTemplates: [
      "portableConfigPage.modules.{moduleId}",
      "portableConfigPage.subranges.{subrangeId}",
    ],
    values: [],
    placeholders: {
      moduleId: [
        "provider_profile",
        "api_keys",
        "cost_catalogs",
        "cost_bindings",
      ],
      subrangeId: [
        "provider_core",
        "provider_keys",
        "provider_models",
        "provider_request_patches",
        "provider_reasoning_config",
        "api_key_core",
        "api_key_acl",
        "api_key_model_override",
        "cost_catalog_core",
        "cost_catalog_versions",
        "cost_components",
        "cost_model_bindings",
      ],
    },
    valueSource:
      "KnownPortableModuleId and KnownPortableSubrangeId; unknown backend IDs use backend labels",
  },
  {
    id: "record-detail-tabs",
    keyTemplates: ["{tab.labelKey}"],
    values: [],
    valueSource: "front/src/pages/record/composables/useRecordDetail.ts RECORD_DETAIL_TABS",
  },
] as const satisfies readonly DynamicI18nKeySource[];

export const DYNAMIC_I18N_FALLBACK_EXCEPTIONS = [
  {
    id: "record-replay-unavailable-reason",
    keyTemplate: "recordPage.detailDialog.replay.reasons.{reason}",
    guard: "te(key)",
    fallback: "raw backend reason",
  },
  {
    id: "portable-config-unknown-module-or-subrange",
    keyTemplate:
      "portableConfigPage.modules.{moduleId} / portableConfigPage.subranges.{subrangeId}",
    guard: "known ID check",
    fallback: "backend-provided label",
  },
] as const satisfies readonly DynamicI18nFallbackException[];
