import test from "node:test";
import assert from "node:assert/strict";

import {
  buildProviderRuntimeSnapshotQuery,
  buildRecordListQuery,
} from "../src/services/query.ts";

test("record list query supports paging and request-level filters", () => {
  assert.equal(
    buildRecordListQuery({
      page: 2,
      page_size: 25,
      estimated_cost_nanos_min: 1000,
      search: "openai",
      final_error_code: "",
    }),
    "page=2&page_size=25&estimated_cost_nanos_min=1000&search=openai",
  );
});

test("provider runtime query keeps window, sort, and only enabled filters", () => {
  assert.equal(
    buildProviderRuntimeSnapshotQuery({
      window: "1h",
      status: "degraded",
      sort: "latency",
      direction: "desc",
      only_enabled: true,
      search: "",
    }),
    "window=1h&status=degraded&sort=latency&direction=desc&only_enabled=true",
  );
});
