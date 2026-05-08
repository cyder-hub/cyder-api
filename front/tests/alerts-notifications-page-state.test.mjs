import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

import {
  buildAlertListParams,
  buildAlertSummaryCounts,
  createDefaultAlertFilters,
  filterAlertsByQuery,
  isAlertSuppressed,
  parseAlertDateTimeLocal,
} from "../src/pages/alerts/composables/alertViewModel.ts";
import {
  buildNotificationDeliveryParams,
  buildNotificationSummaryCounts,
  createNotificationChannelDraft,
  emptyNotificationChannelDraft,
  normalizeNotificationCooldownDraft,
  normalizeNotificationHeadersDraft,
} from "../src/pages/notifications/composables/notificationViewModel.ts";

const ROOT = new URL("../", import.meta.url);

function buildAlert(overrides = {}) {
  return {
    id: 1,
    fingerprint: "fp-provider-a",
    rule_key: "provider_error_rate",
    severity: "warning",
    status: "active",
    scope_type: "provider",
    scope_id: "provider-a",
    title: "Provider error rate",
    summary: "Provider A error rate is elevated.",
    details_json: "{}",
    metrics_snapshot_json: null,
    first_seen_at: 1,
    last_seen_at: 2,
    resolved_at: null,
    acknowledged_at: null,
    acknowledged_note: null,
    suppressed_until: null,
    suppressed_reason: null,
    occurrence_count: 1,
    reopened_count: 0,
    last_notification_at: null,
    created_at: 1,
    updated_at: 2,
    ...overrides,
  };
}

function buildChannel(overrides = {}) {
  return {
    id: 8,
    channel_key: "ops-webhook",
    channel_type: "webhook",
    name: "Ops Webhook",
    endpoint_url: "https://example.test/hook",
    signing_secret_redacted: "sec...",
    headers_json: "{\"X-Team\":\"ops\"}",
    cooldown_seconds: 900,
    is_enabled: true,
    last_test_at: null,
    last_test_success: null,
    last_test_error: null,
    created_at: 1,
    updated_at: 2,
    ...overrides,
  };
}

function buildDelivery(overrides = {}) {
  return {
    id: 10,
    channel_id: 8,
    alert_id: 1,
    alert_fingerprint: "fp-provider-a",
    event_type: "alert_fired",
    status: "failed",
    payload_json: "{}",
    attempt_count: 1,
    next_attempt_at: 5,
    last_attempt_at: 4,
    delivered_at: null,
    last_status_code: 500,
    last_error: "webhook failed",
    created_at: 1,
    updated_at: 2,
    ...overrides,
  };
}

test("alert filters build backend params and local search stays client-side", () => {
  const filters = createDefaultAlertFilters();
  filters.severity = "critical";
  filters.scope_type = "provider";
  filters.acknowledged = "no";
  filters.suppressed = "yes";
  filters.query = "provider-a";

  assert.deepEqual(buildAlertListParams(filters, { limit: 25, offset: 50 }), {
    status: "active",
    severity: "critical",
    scope_type: "provider",
    acknowledged: false,
    suppressed: true,
    limit: 25,
    offset: 50,
  });

  const alerts = [
    buildAlert({ id: 1, scope_id: "provider-a" }),
    buildAlert({
      id: 2,
      fingerprint: "fp-provider-b",
      scope_id: "provider-b",
      title: "Different",
      summary: "Other provider is healthy.",
    }),
  ];
  assert.deepEqual(
    filterAlertsByQuery(alerts, "provider-a").map((alert) => alert.id),
    [1],
  );
});

test("alert summary counts active, critical, suppressed, and acknowledged alerts", () => {
  const now = 1_000;
  const alerts = [
    buildAlert({ id: 1, severity: "critical", suppressed_until: 2_000 }),
    buildAlert({ id: 2, severity: "warning", acknowledged_at: 900 }),
    buildAlert({ id: 3, status: "resolved", severity: "critical" }),
  ];

  assert.equal(isAlertSuppressed(alerts[0], now), true);
  assert.equal(isAlertSuppressed(alerts[0], 3_000), false);
  assert.deepEqual(buildAlertSummaryCounts(alerts, now), {
    active: 2,
    critical: 2,
    suppressed: 1,
    acknowledged: 1,
  });
  assert.equal(parseAlertDateTimeLocal("not-a-date"), null);
});

test("notification channel drafts normalize headers and cooldown fields", () => {
  assert.deepEqual(emptyNotificationChannelDraft(), {
    channel_key: "",
    name: "",
    endpoint_url: "",
    signing_secret: "",
    headers_json: "",
    clear_headers: false,
    cooldown_seconds: "900",
    clear_signing_secret: false,
    is_enabled: true,
  });

  assert.deepEqual(createNotificationChannelDraft(buildChannel()), {
    channel_key: "ops-webhook",
    name: "Ops Webhook",
    endpoint_url: "https://example.test/hook",
    signing_secret: "",
    headers_json: "{\"X-Team\":\"ops\"}",
    clear_headers: false,
    cooldown_seconds: "900",
    clear_signing_secret: false,
    is_enabled: true,
  });

  assert.deepEqual(normalizeNotificationHeadersDraft(""), {
    valid: true,
    value: null,
  });
  assert.deepEqual(normalizeNotificationHeadersDraft('{"X-Team":"ops"}'), {
    valid: true,
    value: "{\"X-Team\":\"ops\"}",
  });
  assert.deepEqual(normalizeNotificationHeadersDraft("[]"), {
    valid: false,
    issue: "headers_invalid",
  });
  assert.deepEqual(normalizeNotificationCooldownDraft(""), {
    valid: true,
    value: 900,
  });
  assert.deepEqual(normalizeNotificationCooldownDraft("86401"), {
    valid: false,
    issue: "cooldown_invalid",
  });
});

test("notification delivery filters and summary counts stay page-local", () => {
  assert.deepEqual(buildNotificationDeliveryParams("failed", "8"), {
    status: "failed",
    channel_id: 8,
    limit: 50,
  });
  assert.deepEqual(buildNotificationDeliveryParams("all", "all"), {
    status: undefined,
    channel_id: undefined,
    limit: 50,
  });

  assert.deepEqual(
    buildNotificationSummaryCounts(
      [buildChannel(), buildChannel({ id: 9, is_enabled: false })],
      [
        buildDelivery({ status: "failed" }),
        buildDelivery({ id: 11, status: "retry_scheduled" }),
      ],
    ),
    {
      channels: 2,
      enabled: 1,
      failed: 1,
      retrying: 1,
    },
  );
});

test("alerts and notifications use page-local entry points", async () => {
  const routerSource = await readFile(new URL("src/router/index.ts", ROOT), "utf8");

  assert.match(routerSource, /pages\/alerts\/AlertsPage\.vue/);
  assert.match(routerSource, /pages\/notifications\/NotificationsPage\.vue/);
  assert.equal(routerSource.includes("@/pages/Alerts.vue"), false);
  assert.equal(routerSource.includes("@/pages/Notification.vue"), false);

  await assert.rejects(() => access(new URL("src/pages/Alerts.vue", ROOT)));
  await assert.rejects(() => access(new URL("src/pages/Notification.vue", ROOT)));
});
