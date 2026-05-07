# Cyder Frontend Development Guide

This frontend is the single-administrator management console for the Cyder LLM
gateway. It is an operator console, not a multi-tenant SaaS application. New
work should prioritize gateway health, request troubleshooting, provider
runtime visibility, API key governance, cost visibility, alerting, and manager
session safety.

## Stack

- Vue 3 with Composition API
- TypeScript
- Vite
- Pinia
- Vue Router
- Tailwind CSS 4
- reka-ui / radix-vue primitives
- lucide-vue-next icons
- vue-i18n

## Page-First Structure

The frontend uses pages + feature first organization. A routed page is the
default business boundary. Page-private state, components, and types stay under
that page directory.

New or refactored pages should use this shape:

```text
front/src/pages/<page>/
  <PageName>Page.vue
  composables/
    use<Page><Feature>.ts
  components/
  types.ts
```

Rules:

- The routed SFC is the composition root for layout, component wiring, event
  binding, and connecting composables to the template.
- Page logic is organized by user workflow or business capability in `useXxx`
  composables, for example `useRecordReplay`, `useApiKeyGovernance`, or
  `useProviderRuntimeFilters`.
- Do not create mechanical horizontal layers such as `state.ts`, `query.ts`,
  `viewModel.ts`, or `format.ts` as the default page structure.
- Do not split code by generic data/view/logic folders. Split by page and
  feature ownership.
- Each page directory owns a `types.ts` file for page-local display types,
  composable return types, component props, and emit contracts.
- Pinia stores are only for state shared across pages. Single-page state belongs
  in page composables.

Top-level `front/src/pages/*.vue` files are legacy migration targets. New routed
work should prefer `front/src/pages/<page>/<PageName>Page.vue`.

## Components

Use shared UI primitives before introducing ad hoc controls:

- `front/src/components/ui`
- `Button`, `Input`, `Select`, `Dialog`, `Table`, `Checkbox`, `Card`, `Badge`,
  `Pagination`, `Popover`, and `Toast`

Component ownership rules:

- Page-private components must live in `front/src/pages/<page>/components/`.
- Cross-page visual components can live in `front/src/components/` only when
  they are reused by multiple pages or are truly generic layout/control pieces.
- Shared components must not call manager APIs directly. Remote actions should
  be passed through props, emits, or page composables.
- Request patch and reasoning UI that is shared between Provider and Model
  belongs in dedicated shared component folders such as
  `front/src/components/request-patch/` and `front/src/components/reasoning/`.

## Services And DTOs

`front/src/services/` owns manager API clients, auth/session behavior, and HTTP
client setup.

Expected service shape:

```text
front/src/services/
  http.ts
  auth.ts
  authTokens.ts
  dashboard.ts
  providers.ts
  models.ts
  modelRoutes.ts
  apiKeys.ts
  records.ts
  providerRuntime.ts
  alerts.ts
  notifications.ts
  cost.ts
  systemConfig.ts
  requestPatch.ts
  types/
```

Rules:

- New business APIs must not be added to `front/src/services/request.ts`.
- Do not grow the global `Api` object. Add or use the domain service that owns
  the manager API area.
- New API DTOs must not be added to `front/src/store/types.ts`.
- API contracts should live in a matching service file or in
  `front/src/services/types/<domain>.ts`.
- Stores may import service DTOs, but stores should not become the DTO
  ownership layer.

## Utils

`front/src/utils/` is the home for pure, cross-page utilities.

Expected utility shape:

```text
front/src/utils/
  cn.ts
  datetime.ts
  money.ts
  error.ts
  clipboard.ts
  sse.ts
```

Rules:

- New global utilities must go under `front/src/utils/`.
- `front/src/lib/*` must not grow with new pure utility code.
- Page-private formatting stays in the owning page composable. Only utilities
  reused across pages belong in `utils`.
- Date, number, and money formatting should use explicit locale inputs instead
  of relying on implicit runtime defaults.

## Store Boundary

Pinia is reserved for cross-page state such as authentication, shared provider
data, shared model data, or shared API key data. Do not add page-only state to
`front/src/store/`.

When a page needs complex state:

1. Put reactive state, computed values, watchers, query synchronization, and
   actions in a page composable.
2. Export pure helpers from that composable or a page-local helper only when
   they need focused tests.
3. Keep the routed SFC focused on layout and interaction wiring.

## UI Direction

The console should stay practical and operations-focused:

- Light, minimal black/white/gray visual system
- Dense but readable admin layouts
- Mobile-first responsive behavior
- Local horizontal scrolling only where tables need it
- Popovers for secondary explanations and diagnostics detail
- Icons from `lucide-vue-next` for icon buttons where possible

Do not introduce RBAC, teams, workspaces, tenant portals, or end-user billing
screens unless explicitly requested.
