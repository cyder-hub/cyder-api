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
      "avg_time_to_first_response_body",
      "avg_ttft",
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
        "avg_time_to_first_response_body",
        "avg_ttft",
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
    id: "record-detail-tabs",
    keyTemplates: ["{tab.labelKey}"],
    values: [],
    valueSource: "front/src/pages/record/composables/useRecordDetail.ts RECORD_DETAIL_TABS",
  },
] as const satisfies readonly DynamicI18nKeySource[];

export const DYNAMIC_I18N_FALLBACK_EXCEPTIONS =
  [] as const satisfies readonly DynamicI18nFallbackException[];
