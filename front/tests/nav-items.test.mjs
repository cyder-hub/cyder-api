import test from "node:test";
import assert from "node:assert/strict";

import { navItems } from "../src/lib/nav-items.ts";

test("navigation keeps Provider as the default start item and groups advanced pages", () => {
  assert.equal(navItems[0].path, "/provider");
  assert.equal(navItems[0].section, "start");

  const byPath = new Map(navItems.map((item) => [item.path, item]));

  assert.equal(byPath.get("/model")?.section, "core");
  assert.equal(byPath.get("/model_route")?.section, "advanced");
  assert.equal(byPath.get("/cost")?.section, "advanced");
  assert.equal(byPath.get("/api_key")?.section, "advanced");
  assert.equal(byPath.has("/custom_fields"), false);
});
