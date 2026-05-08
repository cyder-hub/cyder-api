import test from "node:test";
import assert from "node:assert/strict";

import {
  buildAlertListQuery,
  buildNotificationDeliveryListQuery,
  buildProviderRuntimeListQuery,
  buildProviderRuntimeSummaryQuery,
  buildRecordListQuery,
  buildSystemConfigHistoryQuery,
} from "../src/services/query.ts";

test("alert list query omits empty filters and preserves booleans", () => {
  assert.equal(
    buildAlertListQuery({
      status: "active",
      acknowledged: false,
      suppressed: null,
      severity: "",
      limit: 50,
    }),
    "status=active&acknowledged=false&limit=50",
  );
});

test("notification delivery query keeps selected channel and status filters", () => {
  assert.equal(
    buildNotificationDeliveryListQuery({
      channel_id: 7,
      status: "retry_scheduled",
      alert_id: undefined,
      offset: 20,
    }),
    "channel_id=7&status=retry_scheduled&offset=20",
  );
});

test("record list query supports paging, diagnostics, and cost filters", () => {
  assert.equal(
    buildRecordListQuery({
      page: 2,
      page_size: 25,
      has_transform_diagnostics: true,
      estimated_cost_nanos_min: 1000,
      search: "openai",
      final_error_code: "",
    }),
    "page=2&page_size=25&has_transform_diagnostics=true&estimated_cost_nanos_min=1000&search=openai",
  );
});

test("provider runtime query keeps window, sort, and only enabled filters", () => {
  assert.equal(
    buildProviderRuntimeListQuery({
      window: "1h",
      status: "degraded",
      sort: "latency",
      direction: "desc",
      only_enabled: true,
      search: "",
    }),
    "window=1h&status=degraded&sort=latency&direction=desc&only_enabled=true",
  );
  assert.equal(buildProviderRuntimeSummaryQuery("24h"), "window=24h");
  assert.equal(buildProviderRuntimeSummaryQuery(), "");
});

test("system config history query only includes explicit pagination", () => {
  assert.equal(buildSystemConfigHistoryQuery({ limit: 10 }), "limit=10");
  assert.equal(
    buildSystemConfigHistoryQuery({ limit: 10, offset: 30 }),
    "limit=10&offset=30",
  );
});
