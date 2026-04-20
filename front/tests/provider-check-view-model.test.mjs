import test from "node:test";
import assert from "node:assert/strict";

import { buildCheckOptions } from "../src/composables/providerCheckViewModel.ts";

test("buildCheckOptions keeps selection empty until the user chooses a target", () => {
  const result = buildCheckOptions(["first", "second"], (item, index) => {
    return `${item}-${index}`;
  });

  assert.deepEqual(result, {
    options: [
      { value: 0, label: "#1 first-0" },
      { value: 1, label: "#2 second-1" },
    ],
    defaultSelectedValue: null,
  });
});
