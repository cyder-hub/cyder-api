#!/usr/bin/env node

import { readdir, readFile } from "node:fs/promises";
import { extname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const FRONT_ROOT = fileURLToPath(new URL("../", import.meta.url));
const SRC_ROOT = join(FRONT_ROOT, "src");
const TESTS_ROOT = join(FRONT_ROOT, "tests");
const LOCALES = ["en", "zh"];

const errors = [];
const warnings = [];

function normalizePath(path) {
  return relative(FRONT_ROOT, path).split("\\").join("/");
}

function addError(message) {
  errors.push(message);
}

function addWarning(message) {
  warnings.push(message);
}

async function readJson(path) {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch (error) {
    addError(`${normalizePath(path)} is not valid JSON: ${error.message}`);
    return null;
  }
}

function flattenJson(value, source, prefix = "", output = new Map()) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    addError(`${source} must contain a JSON object at ${prefix || "<root>"}`);
    return output;
  }

  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof child === "string") {
      output.set(path, child);
    } else if (child && typeof child === "object" && !Array.isArray(child)) {
      flattenJson(child, source, path, output);
    } else {
      addError(`${source}.${path} must be a string or nested object`);
    }
  }

  return output;
}

function interpolationParams(message) {
  return [...message.matchAll(/\{([A-Za-z0-9_]+)\}/g)]
    .map((match) => match[1])
    .sort();
}

function formatList(items) {
  return items.sort().join(", ");
}

function compareKeySets(label, mapsByLocale) {
  const baseLocale = LOCALES[0];
  const baseKeys = new Set(mapsByLocale.get(baseLocale).keys());

  for (const locale of LOCALES.slice(1)) {
    const localeKeys = new Set(mapsByLocale.get(locale).keys());
    const missing = [...baseKeys].filter((key) => !localeKeys.has(key));
    const extra = [...localeKeys].filter((key) => !baseKeys.has(key));

    if (missing.length > 0) {
      addError(`${label}: ${locale} is missing keys: ${formatList(missing)}`);
    }
    if (extra.length > 0) {
      addError(`${label}: ${locale} has extra keys: ${formatList(extra)}`);
    }
  }
}

function checkEmptyValues(label, mapsByLocale) {
  for (const [locale, map] of mapsByLocale) {
    for (const [key, value] of map) {
      if (value.trim() === "") {
        addError(`${label}: ${locale}.${key} is empty`);
      }
    }
  }
}

function checkInterpolation(mapsByLocale) {
  const base = mapsByLocale.get("en");
  for (const [key, enValue] of base) {
    const enParams = interpolationParams(enValue);
    for (const locale of LOCALES.slice(1)) {
      const value = mapsByLocale.get(locale).get(key);
      if (typeof value !== "string") continue;
      const localeParams = interpolationParams(value);
      if (enParams.join(",") !== localeParams.join(",")) {
        addError(
          `messages: interpolation mismatch for ${key}: en={${enParams.join(
            ", ",
          )}} ${locale}={${localeParams.join(", ")}}`,
        );
      }
    }
  }
}

function checkGlossaryKeyFormat(glossaryMaps) {
  for (const [locale, map] of glossaryMaps) {
    for (const key of map.keys()) {
      if (!/^[a-z0-9]+(?:_[a-z0-9]+)*$/.test(key)) {
        addError(`glossary: ${locale}.${key} must be snake_case`);
      }
    }
  }
}

async function collectFiles(dir, extensions) {
  const files = [];
  let entries = [];
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return files;
  }

  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectFiles(path, extensions)));
    } else if (extensions.has(extname(path))) {
      files.push(path);
    }
  }

  return files;
}

function lineNumberAt(source, index) {
  return source.slice(0, index).split("\n").length;
}

function looksLikeI18nKey(key) {
  return /^[A-Za-z][\w]*(\.[\w-]+)+$/.test(key);
}

async function checkStaticKeyReferences(messageKeys) {
  const sourceFiles = [
    ...(await collectFiles(SRC_ROOT, new Set([".ts", ".tsx", ".vue"]))),
    ...(await collectFiles(TESTS_ROOT, new Set([".mjs"]))),
  ];
  const patterns = [
    {
      name: "translation call",
      regex:
        /(?:\$t|\bt|\bte|\$te|options\.t|options\.translate|translate)\(\s*["'`]([^"'`$]+)["'`]/g,
    },
    {
      name: "key property",
      regex: /\b(?:i18nKey|titleKey|labelKey):\s*["']([^"']+)["']/g,
    },
    {
      name: "vue key prop",
      regex: /(?:i18n-key|title-key|label-key)=["']([^"']+)["']/g,
    },
  ];

  const missing = [];
  for (const file of sourceFiles) {
    const source = await readFile(file, "utf8");
    for (const { name, regex } of patterns) {
      regex.lastIndex = 0;
      for (const match of source.matchAll(regex)) {
        const key = match[1];
        if (!looksLikeI18nKey(key)) continue;
        if (!messageKeys.has(key)) {
          missing.push(
            `${normalizePath(file)}:${lineNumberAt(source, match.index)} ${name} ${key}`,
          );
        }
      }
    }
  }

  for (const item of missing.sort()) {
    addError(`static key reference missing: ${item}`);
  }
}

function expandTemplate(template, placeholders) {
  const names = [...template.matchAll(/\{([^}]+)\}/g)].map((match) => match[1]);
  if (names.length === 0) return [template];
  if (names.some((name) => !placeholders?.[name])) return [];

  let expanded = [template];
  for (const name of names) {
    const values = placeholders[name];
    expanded = expanded.flatMap((current) =>
      values.map((value) => current.replaceAll(`{${name}}`, value)),
    );
  }
  return expanded;
}

async function checkDynamicKeyCandidates(messageKeys) {
  const modulePath = pathToFileURL(
    join(SRC_ROOT, "i18n/dynamic-key-candidates.ts"),
  ).href;
  const {
    DYNAMIC_I18N_FALLBACK_EXCEPTIONS,
    DYNAMIC_I18N_KEY_SOURCES,
  } = await import(modulePath);

  const requiredIds = [
    "route-title",
    "sidebar-item",
    "sidebar-section",
    "dashboard-usage-metric",
    "request-patch-prefix",
    "alerts",
    "notifications",
    "api-key-governance",
    "api-key-edit-modal",
    "model-capabilities",
    "cost-options",
    "cost-version-state",
    "cost-validation-alert",
    "portable-config-enums",
    "portable-config-known-ids",
    "record-detail-tabs",
  ];
  const sourceIds = new Set(DYNAMIC_I18N_KEY_SOURCES.map((source) => source.id));

  for (const id of requiredIds) {
    if (!sourceIds.has(id)) {
      addError(`dynamic keys: missing source ${id}`);
    }
  }

  for (const source of DYNAMIC_I18N_KEY_SOURCES) {
    if (!source.id || source.keyTemplates.length === 0 || !source.valueSource) {
      addError(`dynamic keys: ${source.id || "<unknown>"} is incomplete`);
      continue;
    }

    for (const template of source.keyTemplates) {
      const expanded = expandTemplate(template, source.placeholders);
      for (const key of expanded) {
        if (looksLikeI18nKey(key) && !messageKeys.has(key)) {
          addError(`dynamic keys: ${source.id} expands to missing key ${key}`);
        }
      }
    }
  }

  const exceptionIds = new Set(
    DYNAMIC_I18N_FALLBACK_EXCEPTIONS.map((exception) => exception.id),
  );
  for (const id of [
    "record-replay-unavailable-reason",
    "portable-config-unknown-module-or-subrange",
  ]) {
    if (!exceptionIds.has(id)) {
      addError(`dynamic keys: missing fallback exception ${id}`);
    }
  }
}

async function checkOldTsDictionarySources() {
  const files = [
    ...(await collectFiles(SRC_ROOT, new Set([".ts", ".vue"]))),
    ...(await collectFiles(TESTS_ROOT, new Set([".mjs"]))),
  ];
  const oldSourceImportPattern =
    /(?:from|import)\s*\(?\s*["'][^"']*(?:src\/i18n\/(?:en|zh)(?:\.ts)?|@\/i18n\/(?:en|zh)(?:\.ts)?|i18n\/(?:en|zh)(?:\.ts)?|\.\/(?:en|zh))["']/;

  for (const file of files) {
    const normalized = normalizePath(file);
    const source = await readFile(file, "utf8");

    if (normalized === "src/i18n/en.ts" || normalized === "src/i18n/zh.ts") {
      if (/export\s+const\s+\w+Dict\s*=\s*\{/.test(source)) {
        addError(`${normalized} must not contain an object literal dictionary`);
      }
      if (!/locales\/(en|zh)\/messages\.json/.test(source)) {
        addError(`${normalized} must re-export from JSON messages`);
      }
      continue;
    }

    if (oldSourceImportPattern.test(source)) {
      addError(`${normalized} references old TS dictionary modules`);
    }
  }
}

async function checkHardcodedCandidates() {
  const candidates = [
    {
      file: "src/components/LanguageSwitcher.vue",
      regex: /aria-label="Change language"/g,
      label: "LanguageSwitcher aria label",
    },
    {
      file: "src/components/ui/pagination/PaginationFirst.vue",
      regex: />\s*First\s*</g,
      label: "pagination First label",
    },
    {
      file: "src/components/ui/pagination/PaginationPrevious.vue",
      regex: />\s*Previous\s*</g,
      label: "pagination Previous label",
    },
    {
      file: "src/components/ui/pagination/PaginationNext.vue",
      regex: />\s*Next\s*</g,
      label: "pagination Next label",
    },
    {
      file: "src/components/ui/pagination/PaginationLast.vue",
      regex: />\s*Last\s*</g,
      label: "pagination Last label",
    },
    {
      file: "src/components/ui/pagination/PaginationEllipsis.vue",
      regex: />\s*More pages\s*</g,
      label: "pagination ellipsis sr-only label",
    },
    {
      file: "src/components/ui/dialog/DialogContent.vue",
      regex: />\s*Close\s*</g,
      label: "dialog close sr-only label",
    },
    {
      file: "src/components/ui/dialog/DialogScrollContent.vue",
      regex: />\s*Close\s*</g,
      label: "dialog scroll close sr-only label",
    },
    {
      file: "src/components/ui/dialog/DialogFooter.vue",
      regex: />\s*Close\s*</g,
      label: "dialog footer close button",
    },
  ];

  for (const candidate of candidates) {
    const path = join(FRONT_ROOT, candidate.file);
    let source = "";
    try {
      source = await readFile(path, "utf8");
    } catch {
      continue;
    }
    for (const match of source.matchAll(candidate.regex)) {
      addWarning(
        `hardcoded candidate: ${candidate.file}:${lineNumberAt(
          source,
          match.index,
        )} ${candidate.label}`,
      );
    }
  }
}

function printResults(messageMaps, glossaryMaps) {
  const enMessageCount = messageMaps.get("en").size;
  const zhMessageCount = messageMaps.get("zh").size;
  const enGlossaryCount = glossaryMaps.get("en").size;
  const zhGlossaryCount = glossaryMaps.get("zh").size;

  console.log(
    `i18n:check messages en=${enMessageCount} zh=${zhMessageCount}; glossary en=${enGlossaryCount} zh=${zhGlossaryCount}`,
  );

  for (const warning of warnings.sort()) {
    console.warn(`warning: ${warning}`);
  }
  for (const error of errors.sort()) {
    console.error(`error: ${error}`);
  }

  if (errors.length > 0) {
    console.error(`i18n:check failed with ${errors.length} error(s)`);
    process.exitCode = 1;
  } else {
    console.log(
      `i18n:check passed with ${warnings.length} warning(s)`,
    );
  }
}

const messageMaps = new Map();
const glossaryMaps = new Map();

for (const locale of LOCALES) {
  const messages = await readJson(
    join(SRC_ROOT, "i18n/locales", locale, "messages.json"),
  );
  const glossary = await readJson(
    join(SRC_ROOT, "i18n/locales", locale, "glossary.json"),
  );
  messageMaps.set(
    locale,
    messages ? flattenJson(messages, `${locale}/messages.json`) : new Map(),
  );
  glossaryMaps.set(
    locale,
    glossary ? flattenJson(glossary, `${locale}/glossary.json`) : new Map(),
  );
}

compareKeySets("messages", messageMaps);
compareKeySets("glossary", glossaryMaps);
checkEmptyValues("messages", messageMaps);
checkEmptyValues("glossary", glossaryMaps);
checkInterpolation(messageMaps);
checkGlossaryKeyFormat(glossaryMaps);
await checkStaticKeyReferences(new Set(messageMaps.get("en").keys()));
await checkDynamicKeyCandidates(new Set(messageMaps.get("en").keys()));
await checkOldTsDictionarySources();
await checkHardcodedCandidates();
printResults(messageMaps, glossaryMaps);
