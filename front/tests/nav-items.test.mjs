import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { navItems, navSectionOrder } from "../src/router/nav-items.ts";

const ROOT = new URL("../", import.meta.url);

async function readSource(path) {
  return readFile(new URL(path, ROOT), "utf8");
}

test("navigation starts with Dashboard and uses operator workflow groups", () => {
  assert.equal(navItems[0].path, "/dashboard");
  assert.equal(navItems[0].navKey, "dashboard");
  assert.equal(navItems[0].section, "operations");
  assert.deepEqual(navSectionOrder, [
    "operations",
    "traffic",
    "resources",
    "governance",
  ]);

  const byPath = new Map(navItems.map((item) => [item.path, item]));

  assert.equal(byPath.get("/provider/runtime")?.section, "operations");
  assert.equal(byPath.get("/alerts")?.section, "operations");
  assert.equal(byPath.get("/notifications")?.section, "operations");
  assert.equal(byPath.get("/record")?.section, "traffic");
  assert.equal(byPath.get("/model_route")?.section, "traffic");
  assert.equal(byPath.get("/provider")?.section, "resources");
  assert.equal(byPath.get("/model")?.section, "resources");
  assert.equal(byPath.get("/api_key")?.section, "resources");
  assert.equal(byPath.get("/cost")?.section, "governance");
  assert.equal(byPath.get("/system/config")?.section, "governance");
  assert.equal(byPath.has("/custom_fields"), false);
  assert.equal(navItems.some((item) => item.section === "start"), false);
  assert.equal(navItems.some((item) => item.section === "overview"), false);
  assert.equal(navItems.some((item) => item.section === "core"), false);
  assert.equal(navItems.some((item) => item.section === "advanced"), false);
});

test("router uses Dashboard as the default entry and defines manager route metadata", async () => {
  const routerSource = await readSource("src/router/index.ts");

  assert.match(routerSource, /path:\s*""[\s\S]*redirect:\s*\{\s*name:\s*"Dashboard"\s*\}/);
  assert.equal(routerSource.includes("@/pages/Index.vue"), false);
  assert.match(routerSource, /to\.name === "Login"[\s\S]*next\(\{ name: "Dashboard" \}\)/);

  for (const navGroup of ["operations", "traffic", "resources", "governance"]) {
    assert.match(routerSource, new RegExp(`navGroup: "${navGroup}"`));
  }

  assert.match(
    routerSource,
    /name:\s*"ProviderEdit"[\s\S]*parentNavKey:\s*"provider"/,
  );
  assert.match(routerSource, /name:\s*"ModelEdit"[\s\S]*parentNavKey:\s*"model"/);
  assert.match(routerSource, /titleKey:\s*"dashboard\.title"/);
  assert.match(routerSource, /navKey:\s*"dashboard"/);
});
