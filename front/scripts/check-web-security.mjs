#!/usr/bin/env node

import { readdir, readFile } from "node:fs/promises";
import { extname, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const MANAGER_UI_PREFIX = "/ai/manager/ui/";

export class WebSecurityBuildError extends Error {
  constructor(message) {
    super(message);
    this.name = "WebSecurityBuildError";
  }
}

function attributes(source) {
  const result = new Map();
  const pattern =
    /([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
  for (const match of source.matchAll(pattern)) {
    result.set(
      match[1].toLowerCase(),
      match[2] ?? match[3] ?? match[4] ?? "",
    );
  }
  return result;
}

function requireManagerAssetUrl(value, description, source) {
  if (!value.startsWith(MANAGER_UI_PREFIX)) {
    throw new WebSecurityBuildError(
      `${source}: ${description} must start with ${MANAGER_UI_PREFIX}`,
    );
  }
}

export function validateBuiltHtml(html, source = "dist/index.html") {
  if (/<style\b/i.test(html)) {
    throw new WebSecurityBuildError(`${source}: inline style element is forbidden`);
  }

  const scripts = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi)];
  const scriptStarts = [...html.matchAll(/<script\b/gi)].length;
  if (scripts.length !== scriptStarts) {
    throw new WebSecurityBuildError(
      `${source}: every script element must have an explicit closing tag`,
    );
  }
  if (scripts.length === 0) {
    throw new WebSecurityBuildError(`${source}: no external entry script found`);
  }
  for (const match of scripts) {
    const attrs = attributes(match[1]);
    const src = attrs.get("src");
    if (!src) {
      throw new WebSecurityBuildError(`${source}: script without src is forbidden`);
    }
    if (match[2].trim() !== "") {
      throw new WebSecurityBuildError(`${source}: inline script body is forbidden`);
    }
    requireManagerAssetUrl(src, "script src", source);
  }

  for (const match of html.matchAll(/<link\b([^>]*)>/gi)) {
    const attrs = attributes(match[1]);
    const rel = (attrs.get("rel") ?? "")
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    if (!rel.includes("modulepreload") && !rel.includes("stylesheet")) {
      continue;
    }
    const href = attrs.get("href");
    if (!href) {
      throw new WebSecurityBuildError(
        `${source}: ${rel.join(" ")} link is missing href`,
      );
    }
    requireManagerAssetUrl(href, `${rel.join(" ")} href`, source);
  }
}

export function validateBuiltJavaScript(
  javascript,
  source = "dist/assets/unknown.js",
) {
  if (/\beval\s*\(/.test(javascript)) {
    throw new WebSecurityBuildError(`${source}: eval() is forbidden`);
  }
  if (/\bnew\s+Function\b/.test(javascript)) {
    throw new WebSecurityBuildError(`${source}: new Function is forbidden`);
  }
}

async function collectJavaScript(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectJavaScript(path)));
    } else if (extname(entry.name) === ".js") {
      files.push(path);
    }
  }
  return files;
}

export async function checkWebSecurityBuild(
  distDirectory = resolve(process.cwd(), "dist"),
) {
  const indexPath = join(distDirectory, "index.html");
  let html;
  try {
    html = await readFile(indexPath, "utf8");
  } catch {
    throw new WebSecurityBuildError("dist/index.html is missing or unreadable");
  }
  validateBuiltHtml(html);

  let javascriptFiles;
  try {
    javascriptFiles = await collectJavaScript(distDirectory);
  } catch {
    throw new WebSecurityBuildError("dist directory is missing or unreadable");
  }
  for (const path of javascriptFiles) {
    validateBuiltJavaScript(
      await readFile(path, "utf8"),
      `dist/${relative(distDirectory, path).split("\\").join("/")}`,
    );
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : "";
if (import.meta.url === invokedPath) {
  try {
    await checkWebSecurityBuild();
    console.log("Web security build check passed.");
  } catch (error) {
    console.error(
      error instanceof Error ? error.message : "Web security build check failed.",
    );
    process.exitCode = 1;
  }
}
