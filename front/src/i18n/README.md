# Frontend i18n and Weblate Handoff

This directory owns the frontend translation source for the Cyder manager UI.
The runtime uses `vue-i18n`, while translation collaboration should happen on
the JSON files under `locales/{lang}`.

## Source Files

```text
front/src/i18n/
  locales/
    en/
      messages.json
      glossary.json
    zh/
      messages.json
      glossary.json
```

- `locales/{lang}/messages.json` is the runtime message source.
- `locales/{lang}/glossary.json` is the terminology source for Weblate.
- `en` is the source and fallback language.
- `zh` is the currently maintained target language.
- `en.ts` and `zh.ts` are compatibility re-exports only. Do not add
  translation strings there.

## Weblate Components

Configure two Weblate components for the initial handoff.

| Component | File mask | Monolingual base language file | File format | Notes |
| --- | --- | --- | --- | --- |
| `frontend-messages` | `front/src/i18n/locales/*/messages.json` | `front/src/i18n/locales/en/messages.json` | `JSON nested structure file` | Runtime UI messages. |
| `frontend-glossary` | `front/src/i18n/locales/*/glossary.json` | `front/src/i18n/locales/en/glossary.json` | `JSON nested structure file` | Enable `Use as a glossary`. |

Recommended component settings:

- Source language: English (`en`).
- Language code style: keep plain language codes (`en`, `zh`) so the `*` in the
  file mask maps directly to the directory name.
- Template for new translations: empty, unless the Weblate instance requires a
  project-level default.
- Translation propagation: leave disabled for monolingual messages unless there
  is a deliberate cross-component reuse rule.
- For `frontend-glossary`, enable `Use as a glossary` and allow managing
  strings so maintainers can add terms from Weblate when needed.

This repository has not been connected to a real Weblate instance as part of
this task. The settings above are a handoff contract based on the local file
layout and Weblate 2026.5 documentation.

## Weblate Format Notes

Weblate documents JSON translations as suitable for JavaScript applications and
recommends a monolingual base file for JSON translations. It also documents that
`JSON nested structure file` preserves JSON structure and inserts new dotted
keys into nested objects.

The JSON format does not carry descriptions, explanations, context, locations,
flags, plurals, or read-only string metadata in the files. For this project:

- Put translator-facing context into stable key names and this README.
- Use `glossary.json` for terminology, not for runtime UI text.
- Configure glossary behavior in Weblate UI for `untranslatable`, `forbidden`,
  or `terminology` flags. Do not invent extra fields in `glossary.json`.
- Revisit the glossary file format only if the project needs file-level storage
  for those flags.

References:

- Weblate JSON files: https://docs.weblate.org/en/latest/formats/json.html
- Weblate glossary: https://docs.weblate.org/en/latest/user/glossary.html
- Weblate component settings: https://docs.weblate.org/en/latest/admin/projects.html

## Developer Rules

When adding or changing frontend text:

1. Add or update `locales/en/messages.json` first.
2. Add the matching `locales/zh/messages.json` key in the same nested location.
3. Use `common` only for truly shared generic actions, status, and UI labels.
4. Prefer page or feature namespaces for business text, errors, help text, and
   long descriptions.
5. Use `ui` for shared component defaults such as pagination, dialog, and other
   reusable primitives.
6. Use `useAppI18n()` and typed keys for new Vue code when practical.
7. For dynamic keys, update `dynamic-key-candidates.ts` so `i18n:check` can
   validate the expanded key set.

When adding or changing terminology:

1. Add the snake_case term key to both `locales/en/glossary.json` and
   `locales/zh/glossary.json`.
2. Keep brand, protocol, API, and model family names in English when the
   existing glossary does so.
3. Do not use glossary keys as runtime translation keys.

Before submitting translation or i18n changes, run:

```bash
rtk npm --prefix front run i18n:check
```

For code changes that affect runtime rendering, also run:

```bash
rtk npm --prefix front test
rtk npm --prefix front run build
```
