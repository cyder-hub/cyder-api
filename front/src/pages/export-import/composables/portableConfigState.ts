import type {
  ConflictStrategy,
  PortableApplyResult,
  PortableBlockedItem,
  PortableDangerousPatchConfirmation,
  PortableExportResponse,
  PortableModuleId,
  PortableModuleRegistryItem,
  PortableModuleRegistryResponse,
  PortableModuleSelection,
  PortablePreviewModule,
  PortablePreviewResponse,
  PortableSubrangeId,
} from "@/services/types";

export const PROVIDER_PROFILE_MODULE_ID = "provider_profile";
export const COST_CATALOGS_MODULE_ID = "cost_catalogs";
export const COST_BINDINGS_MODULE_ID = "cost_bindings";
export const API_KEY_MODEL_OVERRIDE_SUBRANGE_ID = "api_key_model_override";
export const DANGEROUS_PATCH_CONFIRMATION_CODE =
  "dangerous_request_patch_confirmation_required";

export type PortableApplyDisabledReasonCode =
  | "no_preview"
  | "top_level_blocking"
  | "no_selected_modules"
  | "missing_reason"
  | "module_blocking"
  | "dangerous_patch_confirmation";

export interface PortableApplyState {
  preview: PortablePreviewResponse | null;
  selectedModules: PortableModuleSelection[];
  conflictStrategy: ConflictStrategy;
  reason: string;
  dangerousPatchConfirmations: PortableDangerousPatchConfirmation[];
}

export interface PortableModuleRow {
  module: PortableModuleRegistryItem;
  checked: boolean;
  selectedSubrangeIds: Set<PortableSubrangeId>;
  disabledReason: string | null;
}

export interface PortablePreviewModuleRow {
  module: PortablePreviewModule;
  checked: boolean;
}

export function createDefaultPortableExportSelections(
  registry: PortableModuleRegistryResponse,
): PortableModuleSelection[] {
  const defaultIds = new Set<PortableModuleId>(
    registry.default_selected_modules.length
      ? registry.default_selected_modules
      : registry.modules
          .filter((module) => module.default_selected)
          .map((module) => module.module_id),
  );

  return enforcePortableExportSelections(
    registry,
    registry.modules
      .filter((module) => defaultIds.has(module.module_id))
      .map((module) => ({
        module_id: module.module_id,
        subranges: defaultPortableSubranges(module),
      })),
  );
}

export function createDefaultPortableImportSelections(
  preview: PortablePreviewResponse,
): PortableModuleSelection[] {
  if (preview.blocking_issues.length > 0) {
    return [];
  }

  const defaultIds = new Set(preview.default_selected_modules);
  return preview.modules
    .filter((module) => defaultIds.has(module.module_id))
    .filter(isPortablePreviewModuleSelectable)
    .map((module) => ({
      module_id: module.module_id,
      subranges: [...module.subranges],
    }));
}

export function buildPortableModuleRows(
  registry: PortableModuleRegistryResponse | null,
  selections: PortableModuleSelection[],
): PortableModuleRow[] {
  if (!registry) {
    return [];
  }

  const selectedIds = selectedPortableModuleIds(selections);
  return registry.modules.map((module) => {
    const selection = selections.find(
      (item) => item.module_id === module.module_id,
    );
    return {
      module,
      checked: !!selection,
      selectedSubrangeIds: new Set(selection?.subranges ?? []),
      disabledReason: getPortableExportDisabledReason(module, selectedIds),
    };
  });
}

export function buildPortablePreviewModuleRows(
  preview: PortablePreviewResponse | null,
  selections: PortableModuleSelection[],
): PortablePreviewModuleRow[] {
  if (!preview) {
    return [];
  }

  const selectedIds = selectedPortableModuleIds(selections);
  return preview.modules.map((module) => ({
    module,
    checked: selectedIds.has(module.module_id),
  }));
}

export function togglePortableModuleSelection(
  registry: PortableModuleRegistryResponse,
  selections: PortableModuleSelection[],
  moduleId: PortableModuleId,
  checked: boolean,
): PortableModuleSelection[] {
  const module = findPortableRegistryModule(registry, moduleId);
  if (!module) {
    return selections;
  }

  const withoutModule = selections.filter(
    (selection) => selection.module_id !== moduleId,
  );
  if (!checked) {
    return enforcePortableExportSelections(registry, withoutModule);
  }

  const selectedIds = selectedPortableModuleIds(withoutModule);
  selectedIds.add(moduleId);
  if (getPortableExportDisabledReason(module, selectedIds)) {
    return enforcePortableExportSelections(registry, selections);
  }

  return enforcePortableExportSelections(registry, [
    ...withoutModule,
    {
      module_id: module.module_id,
      subranges: defaultPortableSubranges(module),
    },
  ]);
}

export function togglePortableSubrangeSelection(
  registry: PortableModuleRegistryResponse,
  selections: PortableModuleSelection[],
  moduleId: PortableModuleId,
  subrangeId: PortableSubrangeId,
  checked: boolean,
): PortableModuleSelection[] {
  const module = findPortableRegistryModule(registry, moduleId);
  if (!module) {
    return selections;
  }

  const subrange = module.subranges.find(
    (item) => item.subrange_id === subrangeId,
  );
  if (!subrange || subrange.required || subrange.deferred) {
    return selections;
  }

  const currentSelection = selections.find(
    (selection) => selection.module_id === moduleId,
  );
  if (!currentSelection) {
    return selections;
  }

  const subranges = new Set(currentSelection.subranges);
  if (checked) {
    subranges.add(subrangeId);
  } else {
    subranges.delete(subrangeId);
  }

  return enforcePortableExportSelections(
    registry,
    selections.map((selection) =>
      selection.module_id === moduleId
        ? normalizePortableModuleSelection(module, {
            module_id: moduleId,
            subranges: [...subranges],
          })
        : selection,
    ),
  );
}

export function enforcePortableExportSelections(
  registry: PortableModuleRegistryResponse,
  selections: PortableModuleSelection[],
): PortableModuleSelection[] {
  const moduleById = new Map(
    registry.modules.map((module) => [module.module_id, module]),
  );
  const deduped = new Map<PortableModuleId, PortableModuleSelection>();
  for (const selection of selections) {
    if (moduleById.has(selection.module_id)) {
      deduped.set(selection.module_id, selection);
    }
  }

  let next = [...deduped.values()];
  let changed = true;
  while (changed) {
    changed = false;
    const selectedIds = selectedPortableModuleIds(next);
    const filtered = next.filter((selection) => {
      const module = moduleById.get(selection.module_id);
      return !!module && !getPortableExportDisabledReason(module, selectedIds);
    });
    changed = filtered.length !== next.length;
    next = filtered;
  }

  const selectionById = new Map(
    next.map((selection) => [selection.module_id, selection]),
  );
  return registry.modules
    .filter((module) => selectionById.has(module.module_id))
    .map((module) =>
      normalizePortableModuleSelection(
        module,
        selectionById.get(module.module_id),
      ),
    );
}

export function getPortableExportDisabledReason(
  module: PortableModuleRegistryItem,
  selectedModuleIds: Set<PortableModuleId>,
): string | null {
  if (module.deferred) {
    return module.deferred_reason || "deferred";
  }

  const missingDependency = module.dependencies.find(
    (dependency) =>
      dependency.required_for_export &&
      !selectedModuleIds.has(dependency.module_id),
  );
  return missingDependency?.reason ?? null;
}

export function defaultPortableSubranges(
  module: PortableModuleRegistryItem,
): PortableSubrangeId[] {
  return module.subranges
    .filter((subrange) => !subrange.deferred)
    .filter((subrange) => subrange.required || subrange.default_selected)
    .map((subrange) => subrange.subrange_id);
}

export function normalizePortableModuleSelection(
  module: PortableModuleRegistryItem,
  selection?: PortableModuleSelection,
): PortableModuleSelection {
  const selected = new Set(selection?.subranges ?? defaultPortableSubranges(module));
  return {
    module_id: module.module_id,
    subranges: module.subranges
      .filter((subrange) => !subrange.deferred)
      .filter((subrange) => subrange.required || selected.has(subrange.subrange_id))
      .map((subrange) => subrange.subrange_id),
  };
}

export function selectedPortableModuleIds(
  selections: PortableModuleSelection[],
): Set<PortableModuleId> {
  return new Set(selections.map((selection) => selection.module_id));
}

export function findPortableRegistryModule(
  registry: PortableModuleRegistryResponse,
  moduleId: PortableModuleId,
): PortableModuleRegistryItem | undefined {
  return registry.modules.find((module) => module.module_id === moduleId);
}

export function isPortablePreviewModuleSelectable(
  module: PortablePreviewModule,
): boolean {
  return module.supported && module.available && !module.deferred;
}

export function togglePortablePreviewModuleSelection(
  preview: PortablePreviewResponse,
  selections: PortableModuleSelection[],
  moduleId: PortableModuleId,
  checked: boolean,
): PortableModuleSelection[] {
  const module = preview.modules.find((item) => item.module_id === moduleId);
  if (!module || !isPortablePreviewModuleSelectable(module)) {
    return selections;
  }

  const withoutModule = selections.filter(
    (selection) => selection.module_id !== moduleId,
  );
  if (!checked) {
    return withoutModule;
  }

  return [
    ...withoutModule,
    {
      module_id: module.module_id,
      subranges: [...module.subranges],
    },
  ].sort(
    (left, right) =>
      preview.modules.findIndex((module) => module.module_id === left.module_id) -
      preview.modules.findIndex((module) => module.module_id === right.module_id),
  );
}

export function flattenPortableBlockingIssues(
  preview: PortablePreviewResponse | null,
): PortableBlockedItem[] {
  if (!preview) {
    return [];
  }

  return [
    ...preview.blocking_issues,
    ...preview.modules.flatMap((module) => module.blocking_issues),
  ];
}

export function hasPortableBlockingState(
  preview: PortablePreviewResponse | null,
): boolean {
  return flattenPortableBlockingIssues(preview).length > 0;
}

export function mergeDangerousPatchConfirmations(
  preview: PortablePreviewResponse | null,
  existing: PortableDangerousPatchConfirmation[],
): PortableDangerousPatchConfirmation[] {
  const existingByKey = new Map(
    existing.map((confirmation) => [
      dangerousPatchConfirmationKey(confirmation.path, confirmation.target),
      confirmation,
    ]),
  );

  return collectDangerousPatchIssues(preview).map((issue) => {
    const target = issue.target ?? "";
    const key = dangerousPatchConfirmationKey(issue.path, target);
    return {
      path: issue.path,
      target,
      confirmed: existingByKey.get(key)?.confirmed ?? false,
    };
  });
}

export function updateDangerousPatchConfirmation(
  confirmations: PortableDangerousPatchConfirmation[],
  path: string,
  target: string,
  confirmed: boolean,
): PortableDangerousPatchConfirmation[] {
  const key = dangerousPatchConfirmationKey(path, target);
  return confirmations.map((confirmation) =>
    dangerousPatchConfirmationKey(confirmation.path, confirmation.target) === key
      ? { ...confirmation, confirmed }
      : confirmation,
  );
}

export function canApplyPortableImport(state: PortableApplyState): boolean {
  return getPortableApplyDisabledReasonCode(state) === null;
}

export function getPortableApplyDisabledReasonCode(
  state: PortableApplyState,
): PortableApplyDisabledReasonCode | null {
  if (!state.preview) {
    return "no_preview";
  }

  if (
    state.preview.blocking_issues.some(
      (issue) =>
        !isPortableApplyIssueIgnorable(
          issue,
          state.conflictStrategy,
          state.dangerousPatchConfirmations,
        ),
    )
  ) {
    return "top_level_blocking";
  }

  if (state.selectedModules.length === 0) {
    return "no_selected_modules";
  }

  if (!state.reason.trim()) {
    return "missing_reason";
  }

  const selectedIds = selectedPortableModuleIds(state.selectedModules);
  const blockingIssue = state.preview.modules
    .filter((module) => selectedIds.has(module.module_id))
    .flatMap((module) => module.blocking_issues)
    .find(
      (issue) =>
        !isPortableApplyIssueIgnorable(
          issue,
          state.conflictStrategy,
          state.dangerousPatchConfirmations,
        ),
    );

  if (!blockingIssue) {
    return null;
  }

  return blockingIssue.code === DANGEROUS_PATCH_CONFIRMATION_CODE
    ? "dangerous_patch_confirmation"
    : "module_blocking";
}

export function summarizePortableApplyResult(
  result: PortableApplyResult | null,
): string {
  if (!result) {
    return "";
  }
  const { total, create, update, skip, blocked, conflict } = result.summary;
  return [
    `total=${total}`,
    `create=${create}`,
    `update=${update}`,
    `skip=${skip}`,
    `blocked=${blocked}`,
    `conflict=${conflict}`,
  ].join(" ");
}

export function buildPortableDownloadFilename(
  filename: string | null | undefined,
): string {
  const fallback = "cyder-portable-config.cyd";
  const raw = filename?.trim();
  if (!raw) {
    return fallback;
  }

  const basename = raw.split(/[\\/]/).pop() || fallback;
  const sanitized = basename
    .replace(/[\u0000-\u001f\u007f]/g, "")
    .replace(/[<>:"|?*]/g, "-")
    .trim();
  if (!sanitized) {
    return fallback;
  }

  return sanitized.endsWith(".cyd") ? sanitized : `${sanitized}.cyd`;
}

export function downloadPortableExport(response: PortableExportResponse): string {
  const filename = buildPortableDownloadFilename(response.filename);
  const blob = new Blob([response.content], {
    type:
      response.file_protection === "plaintext"
        ? "application/json;charset=utf-8"
        : "text/plain;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  URL.revokeObjectURL(url);
  return filename;
}

function collectDangerousPatchIssues(
  preview: PortablePreviewResponse | null,
): PortableBlockedItem[] {
  return flattenPortableBlockingIssues(preview).filter(
    (issue) => issue.code === DANGEROUS_PATCH_CONFIRMATION_CODE,
  );
}

function dangerousPatchConfirmationKey(path: string, target: string): string {
  return `${path}\u0000${target}`;
}

function isPortableApplyIssueIgnorable(
  issue: PortableBlockedItem,
  conflictStrategy: ConflictStrategy,
  dangerousPatchConfirmations: PortableDangerousPatchConfirmation[],
): boolean {
  if (issue.code === "conflict" && conflictStrategy !== "fail_on_conflict") {
    return true;
  }

  if (
    issue.code === "missing_dependency" &&
    issue.subrange_id === API_KEY_MODEL_OVERRIDE_SUBRANGE_ID
  ) {
    return true;
  }

  if (issue.code === DANGEROUS_PATCH_CONFIRMATION_CODE) {
    const target = issue.target ?? "";
    return dangerousPatchConfirmations.some(
      (confirmation) =>
        confirmation.confirmed &&
        dangerousPatchConfirmationKey(confirmation.path, confirmation.target) ===
          dangerousPatchConfirmationKey(issue.path, target),
    );
  }

  return false;
}
