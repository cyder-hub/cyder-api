import test from "node:test";
import assert from "node:assert/strict";

import {
  validateBuiltHtml,
  validateBuiltJavaScript,
  WebSecurityBuildError,
} from "../scripts/check-web-security.mjs";

const VALID_HTML = `<!doctype html>
<html>
  <head>
    <script type="module" src="/ai/manager/ui/assets/index-ABC123.js"></script>
    <link rel="modulepreload" href="/ai/manager/ui/assets/vendor-ABC123.js">
    <link rel="stylesheet" href="/ai/manager/ui/assets/index-ABC123.css">
  </head>
  <body><div id="app"></div></body>
</html>`;

test("web security build accepts external manager-prefixed assets", () => {
  assert.doesNotThrow(() => validateBuiltHtml(VALID_HTML, "valid.html"));
  assert.doesNotThrow(() =>
    validateBuiltJavaScript("const value = 1;", "valid.js"),
  );
});

test("web security build rejects inline script and style fixtures", () => {
  for (const fixture of [
    `<script>globalThis.compromised = true</script>`,
    `<script src="/ai/manager/ui/assets/app.js">inline()</script>`,
    `<script src="/ai/manager/ui/assets/app.js" />`,
    `<style>body { display: none }</style>${VALID_HTML}`,
  ]) {
    assert.throws(
      () => validateBuiltHtml(fixture, "inline-negative.html"),
      WebSecurityBuildError,
    );
  }
});

test("web security build rejects external and misplaced entry assets", () => {
  for (const fixture of [
    `<script src="https://cdn.example/app.js"></script>`,
    `<script src="/assets/app.js"></script>`,
    `<script src="/ai/manager/ui/assets/app.js"></script><link rel="stylesheet" href="/assets/app.css">`,
    `<script src="/ai/manager/ui/assets/app.js"></script><link rel="modulepreload" href="//cdn.example/vendor.js">`,
  ]) {
    assert.throws(
      () => validateBuiltHtml(fixture, "asset-negative.html"),
      WebSecurityBuildError,
    );
  }
});

test("web security build rejects dynamic JavaScript evaluation fixtures", () => {
  for (const fixture of [
    `const result = eval("1 + 1");`,
    `const factory = new Function("return 1");`,
  ]) {
    assert.throws(
      () => validateBuiltJavaScript(fixture, "javascript-negative.js"),
      WebSecurityBuildError,
    );
  }
});
