import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

import {
  resolveCostCatalogSelection,
  resolveCostVersionSelection,
  resolvePreferredCostVersionId,
} from "../src/pages/cost/composables/useCostCatalogs.ts";
import { buildCostPreviewNormalization } from "../src/pages/cost/composables/useCostPreview.ts";

const ROOT = new URL("../", import.meta.url);

const catalogItems = [
  {
    catalog: {
      id: 10,
      name: "OpenAI",
      description: null,
      created_at: 1,
      updated_at: 1,
    },
    versions: [
      {
        id: 100,
        catalog_id: 10,
        version: "2026-01",
        currency: "USD",
        source: "manual",
        effective_from: 1,
        effective_until: null,
        first_used_at: null,
        is_archived: false,
        is_enabled: true,
        created_at: 1,
        updated_at: 1,
      },
      {
        id: 101,
        catalog_id: 10,
        version: "2025-12",
        currency: "USD",
        source: "manual",
        effective_from: 1,
        effective_until: null,
        first_used_at: 2,
        is_archived: true,
        is_enabled: false,
        created_at: 1,
        updated_at: 1,
      },
    ],
  },
  {
    catalog: {
      id: 20,
      name: "Anthropic",
      description: null,
      created_at: 1,
      updated_at: 1,
    },
    versions: [],
  },
];

test("cost catalog selection keeps valid catalog and falls back to first available", () => {
  assert.equal(resolveCostCatalogSelection(catalogItems, 10), 10);
  assert.equal(resolveCostCatalogSelection(catalogItems, 999), 10);
  assert.equal(resolveCostCatalogSelection(catalogItems, null), 10);
  assert.equal(resolveCostCatalogSelection([], 10), null);
});

test("cost version selection follows selected catalog versions", () => {
  assert.equal(resolveCostVersionSelection(catalogItems, 10, 100), 100);
  assert.equal(resolveCostVersionSelection(catalogItems, 10, 999), 100);
  assert.equal(resolveCostVersionSelection(catalogItems, 20, 100), null);
  assert.equal(resolveCostVersionSelection(catalogItems, null, 100), null);
});

test("cost preferred version selection hides archived versions unless requested", () => {
  const versions = catalogItems[0].versions;

  assert.equal(resolvePreferredCostVersionId(versions, 101, false), 100);
  assert.equal(resolvePreferredCostVersionId(versions, 101, true), 101);
  assert.equal(resolvePreferredCostVersionId(versions, null, false), 100);
  assert.equal(resolvePreferredCostVersionId([versions[1]], null, false), null);
});

test("cost preview normalization parses all token fields as non-negative integers", () => {
  assert.deepEqual(
    buildCostPreviewNormalization({
      total_input_tokens: "1200",
      total_output_tokens: "640",
      input_text_tokens: "1200",
      output_text_tokens: "640",
      input_image_tokens: "0",
      output_image_tokens: "0",
      cache_read_tokens: "10",
      cache_write_tokens: "5",
      reasoning_tokens: "20",
    }),
    {
      total_input_tokens: 1200,
      total_output_tokens: 640,
      input_text_tokens: 1200,
      output_text_tokens: 640,
      input_image_tokens: 0,
      output_image_tokens: 0,
      cache_read_tokens: 10,
      cache_write_tokens: 5,
      reasoning_tokens: 20,
      warnings: [],
    },
  );

  assert.throws(() =>
    buildCostPreviewNormalization({
      total_input_tokens: "not-a-number",
      total_output_tokens: "640",
      input_text_tokens: "1200",
      output_text_tokens: "640",
      input_image_tokens: "0",
      output_image_tokens: "0",
      cache_read_tokens: "10",
      cache_write_tokens: "5",
      reasoning_tokens: "20",
    }),
  );
});

test("cost page uses page-local entry point and no global cost store", async () => {
  const routerSource = await readFile(new URL("src/router/index.ts", ROOT), "utf8");
  const modelEditSource = await readFile(
    new URL("src/pages/model-edit/composables/useModelEdit.ts", ROOT),
    "utf8",
  );

  assert.match(routerSource, /pages\/cost\/CostPage\.vue/);
  assert.equal(routerSource.includes("@/pages/Cost.vue"), false);
  assert.equal(modelEditSource.includes("@/store/costStore"), false);
  assert.equal(modelEditSource.includes("costStore"), false);

  await assert.rejects(() => access(new URL("src/pages/Cost.vue", ROOT)));
  await assert.rejects(() => access(new URL("src/store/costStore.ts", ROOT)));
});
