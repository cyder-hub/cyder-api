<template>
  <section class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div class="min-w-0">
        <h3 class="text-lg font-semibold text-gray-900">
          {{ t("modelEditPage.requestPatch.title") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ t("modelEditPage.requestPatch.description") }}
        </p>
      </div>
      <Button
        variant="ghost"
        size="sm"
        class="w-full sm:w-auto"
        :disabled="isLoading || isRefreshing"
        @click="handleRefresh"
      >
        <RefreshCw
          class="mr-1.5 h-4 w-4"
          :class="{ 'animate-spin': isRefreshing }"
        />
        {{ t("modelEditPage.requestPatch.refresh") }}
      </Button>
    </div>

    <div v-if="isLoading" class="flex items-center justify-center py-16">
      <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
      <span class="text-sm text-gray-500">
        {{ t("modelEditPage.requestPatch.loading") }}
      </span>
    </div>

    <div
      v-else-if="loadError"
      class="mt-4 rounded-lg border border-red-200 bg-red-50 px-4 py-5"
    >
      <p class="text-sm font-medium text-red-700">
        {{ loadError }}
      </p>
      <Button class="mt-3" variant="outline" size="sm" @click="handleRefresh">
        {{ t("common.retry") }}
      </Button>
    </div>

    <div v-else class="mt-5 space-y-6">
      <div
        v-if="hasConflicts"
        class="rounded-lg border border-red-200 bg-red-50 px-4 py-4"
      >
        <div class="flex items-start gap-3">
          <ShieldAlert class="mt-0.5 h-5 w-5 shrink-0 text-red-600" />
          <div class="min-w-0 flex-1">
            <p class="text-sm font-semibold text-red-800">
              {{ t("modelEditPage.requestPatch.conflictBannerTitle") }}
            </p>
            <p class="mt-1 text-sm text-red-700">
              {{ t("modelEditPage.requestPatch.conflictBannerDescription") }}
            </p>
            <div
              class="mt-4 overflow-hidden rounded-lg border border-red-100 bg-white/80"
            >
              <div
                v-for="conflict in conflicts"
                :key="`${conflict.provider_rule_id}-${conflict.model_rule_id}-${conflict.provider_target}-${conflict.model_target}`"
                class="border-t border-red-100 px-4 py-3 first:border-t-0"
              >
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="destructive" class="font-mono text-[11px]">
                    {{ conflict.placement }}
                  </Badge>
                  <Badge variant="outline" class="font-mono text-[11px]">
                    #{{ conflict.provider_rule_id }} -> #{{ conflict.model_rule_id }}
                  </Badge>
                </div>
                <div class="mt-2 grid gap-2 text-sm text-red-800 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-red-600">
                      {{ t("modelEditPage.requestPatch.conflictFields.providerTarget") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-xs text-red-900">
                      {{ conflict.provider_target }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-red-600">
                      {{ t("modelEditPage.requestPatch.conflictFields.modelTarget") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-xs text-red-900">
                      {{ conflict.model_target }}
                    </p>
                  </div>
                </div>
                <p class="mt-2 text-sm text-red-700">
                  {{ conflict.reason }}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="space-y-3">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <h4 class="text-base font-semibold text-gray-900">
              {{ t("modelEditPage.requestPatch.directTitle") }}
            </h4>
            <p class="mt-1 text-sm text-gray-500">
              {{ t("modelEditPage.requestPatch.directDescription") }}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            class="w-full sm:w-auto"
            :disabled="!modelId"
            @click="openCreateDialog"
          >
            <Plus class="mr-1.5 h-4 w-4" />
            {{ t("modelEditPage.requestPatch.addRule") }}
          </Button>
        </div>

        <div
          v-if="directRules.length === 0"
          class="flex flex-col items-center justify-center rounded-lg border border-dashed border-gray-200 py-10"
        >
          <FileText class="mb-2 h-10 w-10 stroke-1 text-gray-400" />
          <span class="text-sm font-medium text-gray-500">
            {{ t("modelEditPage.requestPatch.emptyDirect") }}
          </span>
        </div>

        <div
          v-else
          class="overflow-hidden rounded-lg border border-gray-200 bg-white"
        >
          <div
            v-for="rule in directRules"
            :key="rule.id"
            class="border-t border-gray-100 px-4 py-4 first:border-t-0"
          >
            <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0 flex-1 space-y-3">
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ rule.placement }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-[11px]">
                    {{ rule.operation }}
                  </Badge>
                  <Badge
                    :variant="getDirectRuleState(rule).variant"
                    class="text-[11px]"
                  >
                    {{ getDirectRuleState(rule).label }}
                  </Badge>
                </div>

                <p class="break-all font-mono text-sm text-gray-900">
                  {{ rule.target }}
                </p>

                <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.value") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-sm text-gray-700">
                      {{ formatRequestPatchValueForDisplay(rule.value_json) }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.description") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ rule.description || t("modelEditPage.requestPatch.noDescription") }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.ruleId") }}
                    </p>
                    <p class="mt-1 font-mono text-xs text-gray-600">#{{ rule.id }}</p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.trace") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ getDirectRuleTrace(rule) }}
                    </p>
                  </div>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2 sm:ml-4 sm:justify-end">
                <div
                  class="flex items-center gap-2 rounded-lg border border-gray-200 px-3 py-2"
                >
                  <span class="text-xs font-medium text-gray-500">
                    {{ t("modelEditPage.requestPatch.fields.enabled") }}
                  </span>
                  <Checkbox
                    :model-value="rule.is_enabled"
                    :disabled="isRuleBusy(rule.id)"
                    @update:model-value="(checked) => handleToggleEnabled(rule, checked === true)"
                  />
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-8 px-2 text-gray-600"
                  :disabled="isRuleBusy(rule.id)"
                  @click="openEditDialog(rule)"
                >
                  <Pencil class="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-8 px-2 text-gray-400 hover:text-red-600"
                  :disabled="isRuleBusy(rule.id)"
                  @click="handleDeleteRule(rule)"
                >
                  <Trash2 class="h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="border-t border-gray-100 pt-5">
        <div class="space-y-3">
          <div class="min-w-0">
            <h4 class="text-base font-semibold text-gray-900">
              {{ t("modelEditPage.requestPatch.inheritedTitle") }}
            </h4>
            <p class="mt-1 text-sm text-gray-500">
              {{ t("modelEditPage.requestPatch.inheritedDescription", { provider: providerLabel }) }}
            </p>
          </div>

          <div
            v-if="inheritedRules.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.requestPatch.emptyInherited") }}
          </div>

          <div
            v-else
            class="overflow-hidden rounded-lg border border-gray-200 bg-white"
          >
            <div
              v-for="item in inheritedRules"
              :key="item.rule.id"
              class="border-t border-gray-100 px-4 py-4 first:border-t-0"
            >
              <div class="space-y-3">
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ item.rule.placement }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-[11px]">
                    {{ item.rule.operation }}
                  </Badge>
                  <Badge
                    :variant="getInheritedRuleState(item).variant"
                    class="text-[11px]"
                  >
                    {{ getInheritedRuleState(item).label }}
                  </Badge>
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ t("modelEditPage.requestPatch.origin.ProviderDirect") }}
                  </Badge>
                </div>

                <p class="break-all font-mono text-sm text-gray-900">
                  {{ item.rule.target }}
                </p>

                <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.value") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-sm text-gray-700">
                      {{ formatRequestPatchValueForDisplay(item.rule.value_json) }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.description") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ item.rule.description || t("modelEditPage.requestPatch.noDescription") }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.source") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ providerLabel }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.trace") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ getInheritedRuleTrace(item) }}
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="border-t border-gray-100 pt-5">
        <div class="space-y-3">
          <div class="min-w-0">
            <h4 class="text-base font-semibold text-gray-900">
              {{ t("modelEditPage.requestPatch.effectiveTitle") }}
            </h4>
            <p class="mt-1 text-sm text-gray-500">
              {{ t("modelEditPage.requestPatch.effectiveDescription") }}
            </p>
          </div>

          <div
            v-if="effectiveRules.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.requestPatch.emptyEffective") }}
          </div>

          <div
            v-else
            class="overflow-hidden rounded-lg border border-gray-200 bg-white"
          >
            <div
              v-for="rule in effectiveRules"
              :key="`${rule.source_rule_id}-${rule.target}`"
              class="border-t border-gray-100 px-4 py-4 first:border-t-0"
            >
              <div class="space-y-3">
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ rule.placement }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-[11px]">
                    {{ rule.operation }}
                  </Badge>
                  <Badge class="text-[11px]" :variant="rule.source_origin === 'ModelDirect' ? 'default' : 'secondary'">
                    {{ t(`modelEditPage.requestPatch.origin.${rule.source_origin}`) }}
                  </Badge>
                </div>

                <p class="break-all font-mono text-sm text-gray-900">
                  {{ rule.target }}
                </p>

                <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.value") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-sm text-gray-700">
                      {{ formatRequestPatchValueForDisplay(rule.value_json) }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.description") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ rule.description || t("modelEditPage.requestPatch.noDescription") }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.sourceRule") }}
                    </p>
                    <p class="mt-1 font-mono text-xs text-gray-600">
                      #{{ rule.source_rule_id }}
                    </p>
                  </div>
                  <div>
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.requestPatch.fields.trace") }}
                    </p>
                    <p class="mt-1 text-sm text-gray-600">
                      {{ getEffectiveRuleTrace(rule) }}
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="border-t border-gray-100 pt-5">
        <div class="space-y-3">
          <div class="min-w-0">
            <h4 class="text-base font-semibold text-gray-900">
              {{ t("modelEditPage.requestPatch.explainTitle") }}
            </h4>
            <p class="mt-1 text-sm text-gray-500">
              {{ t("modelEditPage.requestPatch.explainDescription") }}
            </p>
          </div>

          <div
            v-if="explainEntries.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.requestPatch.emptyExplain") }}
          </div>

          <div
            v-else
            class="overflow-hidden rounded-lg border border-gray-200 bg-white"
          >
            <div
              v-for="entry in explainEntries"
              :key="entry.rule.id"
              class="border-t border-gray-100 px-4 py-4 first:border-t-0"
            >
              <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0 flex-1 space-y-3">
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="font-mono text-[11px]">
                      {{ entry.rule.placement }}
                    </Badge>
                    <Badge
                      class="text-[11px]"
                      :variant="entry.origin === 'ModelDirect' ? 'default' : 'secondary'"
                    >
                      {{ t(`modelEditPage.requestPatch.origin.${entry.origin}`) }}
                    </Badge>
                    <Badge
                      :variant="getExplainStatus(entry.status).variant"
                      class="text-[11px]"
                    >
                      {{ getExplainStatus(entry.status).label }}
                    </Badge>
                  </div>

                  <p class="break-all font-mono text-sm text-gray-900">
                    {{ entry.rule.target }}
                  </p>

                  <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                    <div>
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.requestPatch.fields.ruleId") }}
                      </p>
                      <p class="mt-1 font-mono text-xs text-gray-600">#{{ entry.rule.id }}</p>
                    </div>
                    <div>
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.requestPatch.fields.trace") }}
                      </p>
                      <p class="mt-1 text-sm text-gray-600">
                        {{ entry.message || t("modelEditPage.requestPatch.messages.noRuntimeTrace") }}
                      </p>
                    </div>
                  </div>
                </div>

                <Popover
                  v-if="entry.message || entry.effective_rule_id !== null || entry.conflict_with_rule_ids.length > 0"
                >
                  <PopoverTrigger as-child>
                    <Button variant="ghost" size="sm" class="w-full sm:w-auto">
                      {{ t("modelEditPage.requestPatch.details") }}
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent
                    align="end"
                    class="w-80 border-gray-200 bg-white p-3 text-sm text-gray-700"
                  >
                    <div class="space-y-3">
                      <div v-if="entry.message" class="rounded-md bg-gray-50 px-3 py-2">
                        {{ entry.message }}
                      </div>
                      <div v-if="entry.effective_rule_id !== null">
                        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                          {{ t("modelEditPage.requestPatch.fields.effectiveRule") }}
                        </p>
                        <p class="mt-1 font-mono text-xs text-gray-700">
                          #{{ entry.effective_rule_id }}
                        </p>
                      </div>
                      <div v-if="entry.conflict_with_rule_ids.length > 0">
                        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                          {{ t("modelEditPage.requestPatch.fields.conflictsWith") }}
                        </p>
                        <p class="mt-1 font-mono text-xs text-gray-700">
                          {{ formatRuleIds(entry.conflict_with_rule_ids) }}
                        </p>
                      </div>
                    </div>
                  </PopoverContent>
                </Popover>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <Dialog :open="isEditorOpen" @update:open="handleEditorOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-2xl">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{
              editorMode === "create"
                ? t("modelEditPage.requestPatch.editor.addTitle")
                : t("modelEditPage.requestPatch.editor.editTitle")
            }}
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            {{ t("modelEditPage.requestPatch.editor.description") }}
          </DialogDescription>
        </DialogHeader>

        <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ t("modelEditPage.requestPatch.editor.placement") }}
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Select v-model="editorForm.placement">
                <SelectTrigger class="w-full">
                  <SelectValue :placeholder="t('modelEditPage.requestPatch.editor.selectPlacement')" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in placementOptions"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ option.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ t("modelEditPage.requestPatch.editor.operation") }}
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Select v-model="editorForm.operation">
                <SelectTrigger class="w-full">
                  <SelectValue :placeholder="t('modelEditPage.requestPatch.editor.selectOperation')" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in operationOptions"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ option.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ t("modelEditPage.requestPatch.editor.target") }}
              <span class="ml-0.5 text-red-500">*</span>
            </Label>
            <Input
              v-model="editorForm.target"
              :placeholder="targetPlaceholder"
              class="font-mono text-sm"
            />
            <p class="text-xs text-gray-500">
              {{ targetHelpText }}
            </p>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ t("modelEditPage.requestPatch.editor.valueJson") }}
              <span v-if="editorForm.operation === 'SET'" class="ml-0.5 text-red-500">*</span>
            </Label>
            <textarea
              v-model="editorForm.value_json_text"
              :disabled="editorForm.operation === 'REMOVE'"
              :placeholder="valueJsonPlaceholder"
              class="min-h-32 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200 disabled:cursor-not-allowed disabled:bg-gray-50 disabled:text-gray-400"
            />
            <p class="text-xs text-gray-500">
              {{ t("modelEditPage.requestPatch.editor.valueHelp") }}
            </p>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ t("modelEditPage.requestPatch.editor.descriptionLabel") }}
            </Label>
            <textarea
              v-model="editorForm.description"
              :placeholder="t('modelEditPage.requestPatch.editor.descriptionPlaceholder')"
              class="min-h-24 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200"
            />
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <div>
              <p class="text-sm font-medium text-gray-900">
                {{ t("modelEditPage.requestPatch.editor.enabledTitle") }}
              </p>
              <p class="mt-1 text-xs text-gray-500">
                {{ t("modelEditPage.requestPatch.editor.enabledDescription") }}
              </p>
            </div>
            <Checkbox v-model="editorForm.is_enabled" />
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
          <Button
            variant="ghost"
            class="w-full text-gray-600 sm:w-auto"
            :disabled="isSubmitting"
            @click="isEditorOpen = false"
          >
            {{ t("common.cancel") }}
          </Button>
          <Button
            variant="default"
            class="w-full sm:w-auto"
            :disabled="isSubmitting"
            @click="handleSaveRule"
          >
            <Loader2 v-if="isSubmitting" class="mr-1.5 h-4 w-4 animate-spin" />
            {{
              editorMode === "create"
                ? t("modelEditPage.requestPatch.editor.create")
                : t("modelEditPage.requestPatch.editor.save")
            }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="isDangerDialogOpen" @update:open="handleDangerDialogOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-lg">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{ t("modelEditPage.requestPatch.editor.dangerousTitle") }}
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            {{ t("modelEditPage.requestPatch.editor.dangerousDescription") }}
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-4 px-4 py-4 sm:px-6">
          <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-4 py-3">
            <p class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ t("modelEditPage.requestPatch.editor.target") }}
            </p>
            <p class="mt-1 font-mono text-sm text-gray-900">
              {{ pendingDangerousConfirmation?.confirmation.placement }}
              {{ pendingDangerousConfirmation?.confirmation.target }}
            </p>
          </div>

          <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
            {{ pendingDangerousConfirmation?.confirmation.reason }}
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
          <Button
            variant="ghost"
            class="w-full text-gray-600 sm:w-auto"
            :disabled="isConfirmingDangerous"
            @click="cancelDangerousConfirmation"
          >
            {{ t("common.cancel") }}
          </Button>
          <Button
            variant="default"
            class="w-full sm:w-auto"
            :disabled="isConfirmingDangerous"
            @click="confirmDangerousSubmission"
          >
            <Loader2
              v-if="isConfirmingDangerous"
              class="mr-1.5 h-4 w-4 animate-spin"
            />
            {{ t("modelEditPage.requestPatch.editor.saveAnyway") }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { normalizeError } from "@/lib/error";
import {
  buildRequestPatchPayloadFromEditor,
  formatRequestPatchValueForDisplay,
  formatRequestPatchValueForEditor,
} from "@/lib/requestPatch";
import { toastController } from "@/lib/toastController";
import type {
  InheritedRequestPatchRule,
  RequestPatchDangerousTargetConfirmation,
  RequestPatchExplainEntry,
  RequestPatchExplainStatus,
  RequestPatchOperation,
  RequestPatchPayload,
  RequestPatchPlacement,
  RequestPatchRule,
  RequestPatchUpdatePayload,
  ResolvedRequestPatchRule,
  RequestPatchConflict,
} from "@/store/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  FileText,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  ShieldAlert,
  Trash2,
} from "lucide-vue-next";

type EditorMode = "create" | "edit";
type BadgeVariant = "default" | "secondary" | "destructive" | "outline";

interface RequestPatchEditorForm {
  id: number | null;
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json_text: string;
  description: string;
  is_enabled: boolean;
}

interface PendingDangerousSubmission {
  mode: EditorMode;
  ruleId: number | null;
  payload: RequestPatchPayload | RequestPatchUpdatePayload;
  confirmation: RequestPatchDangerousTargetConfirmation;
}

const props = defineProps<{
  modelId: number | null;
  providerName?: string | null;
  providerKey?: string | null;
}>();

const { t } = useI18n();

const placementOptions = computed<
  Array<{ value: RequestPatchPlacement; label: string }>
>(() => [
  {
    value: "HEADER",
    label: t("modelEditPage.requestPatch.placements.HEADER"),
  },
  {
    value: "QUERY",
    label: t("modelEditPage.requestPatch.placements.QUERY"),
  },
  { value: "BODY", label: t("modelEditPage.requestPatch.placements.BODY") },
]);

const operationOptions = computed<
  Array<{ value: RequestPatchOperation; label: string }>
>(() => [
  { value: "SET", label: t("modelEditPage.requestPatch.operations.SET") },
  {
    value: "REMOVE",
    label: t("modelEditPage.requestPatch.operations.REMOVE"),
  },
]);

const isLoading = ref(true);
const isRefreshing = ref(false);
const loadError = ref<string | null>(null);
const directRules = ref<RequestPatchRule[]>([]);
const inheritedRules = ref<InheritedRequestPatchRule[]>([]);
const effectiveRules = ref<ResolvedRequestPatchRule[]>([]);
const explainEntries = ref<RequestPatchExplainEntry[]>([]);
const conflicts = ref<RequestPatchConflict[]>([]);
const hasConflicts = ref(false);

const isEditorOpen = ref(false);
const isDangerDialogOpen = ref(false);
const isSubmitting = ref(false);
const isConfirmingDangerous = ref(false);
const activeRuleId = ref<number | null>(null);
const editorMode = ref<EditorMode>("create");
const pendingDangerousConfirmation = ref<PendingDangerousSubmission | null>(null);
const editorForm = ref<RequestPatchEditorForm>(createEmptyForm());

const providerLabel = computed(() => {
  const name = props.providerName?.trim();
  if (name) {
    return name;
  }

  const key = props.providerKey?.trim();
  if (key) {
    return key;
  }

  return t("modelEditPage.requestPatch.providerFallback");
});

const explainByRuleId = computed(() => {
  const map = new Map<number, RequestPatchExplainEntry>();
  for (const entry of explainEntries.value) {
    map.set(entry.rule.id, entry);
  }
  return map;
});

const targetPlaceholder = computed(() => {
  switch (editorForm.value.placement) {
    case "HEADER":
      return t("modelEditPage.requestPatch.editor.targetPlaceholderHeader");
    case "QUERY":
      return t("modelEditPage.requestPatch.editor.targetPlaceholderQuery");
    case "BODY":
      return t("modelEditPage.requestPatch.editor.targetPlaceholderBody");
  }
});

const targetHelpText = computed(() => {
  switch (editorForm.value.placement) {
    case "HEADER":
      return t("modelEditPage.requestPatch.editor.targetHelpHeader");
    case "QUERY":
      return t("modelEditPage.requestPatch.editor.targetHelpQuery");
    case "BODY":
      return t("modelEditPage.requestPatch.editor.targetHelpBody");
  }
});

const valueJsonPlaceholder = computed(() =>
  editorForm.value.operation === "REMOVE"
    ? t("modelEditPage.requestPatch.editor.removeValuePlaceholder")
    : t("modelEditPage.requestPatch.editor.valuePlaceholder"),
);

function createEmptyForm(): RequestPatchEditorForm {
  return {
    id: null,
    placement: "BODY",
    target: "",
    operation: "SET",
    value_json_text: "",
    description: "",
    is_enabled: true,
  };
}

function clearState() {
  directRules.value = [];
  inheritedRules.value = [];
  effectiveRules.value = [];
  explainEntries.value = [];
  conflicts.value = [];
  hasConflicts.value = false;
}

function formatRuleIds(ruleIds: number[]): string {
  return ruleIds.map((id) => `#${id}`).join(", ");
}

function getExplainEntry(ruleId: number): RequestPatchExplainEntry | null {
  return explainByRuleId.value.get(ruleId) ?? null;
}

function isRuleBusy(ruleId: number): boolean {
  return activeRuleId.value === ruleId;
}

function getExplainStatus(
  status: RequestPatchExplainStatus,
): { label: string; variant: BadgeVariant } {
  switch (status) {
    case "Effective":
      return {
        label: t("modelEditPage.requestPatch.states.effective"),
        variant: "secondary",
      };
    case "Overridden":
      return {
        label: t("modelEditPage.requestPatch.states.overridden"),
        variant: "outline",
      };
    case "Conflicted":
      return {
        label: t("modelEditPage.requestPatch.states.conflicted"),
        variant: "destructive",
      };
  }
}

function getDirectRuleState(rule: RequestPatchRule): {
  label: string;
  variant: BadgeVariant;
} {
  if (!rule.is_enabled) {
    return {
      label: t("modelEditPage.requestPatch.states.disabled"),
      variant: "outline",
    };
  }

  const explainEntry = getExplainEntry(rule.id);
  if (!explainEntry) {
    return {
      label: t("modelEditPage.requestPatch.states.enabled"),
      variant: "secondary",
    };
  }

  return getExplainStatus(explainEntry.status);
}

function getDirectRuleTrace(rule: RequestPatchRule): string {
  if (!rule.is_enabled) {
    return t("modelEditPage.requestPatch.messages.disabledSkipped");
  }

  const explainEntry = getExplainEntry(rule.id);
  if (!explainEntry) {
    return t("modelEditPage.requestPatch.messages.directEffective");
  }

  return explainEntry.message || t("modelEditPage.requestPatch.messages.directEffective");
}

function getInheritedRuleState(item: InheritedRequestPatchRule): {
  label: string;
  variant: BadgeVariant;
} {
  if (item.conflict_with_rule_ids.length > 0) {
    return {
      label: t("modelEditPage.requestPatch.states.conflicted"),
      variant: "destructive",
    };
  }

  if (item.overridden_by_rule_id !== null) {
    return {
      label: t("modelEditPage.requestPatch.states.overridden"),
      variant: "outline",
    };
  }

  return {
    label: t("modelEditPage.requestPatch.states.effective"),
    variant: "secondary",
  };
}

function getInheritedRuleTrace(item: InheritedRequestPatchRule): string {
  if (item.conflict_with_rule_ids.length > 0) {
    return t("modelEditPage.requestPatch.messages.conflictsWithRules", {
      ids: formatRuleIds(item.conflict_with_rule_ids),
    });
  }

  if (item.overridden_by_rule_id !== null) {
    return t("modelEditPage.requestPatch.messages.overriddenByRule", {
      id: `#${item.overridden_by_rule_id}`,
    });
  }

  return t("modelEditPage.requestPatch.messages.inheritedEffective");
}

function getEffectiveRuleTrace(rule: ResolvedRequestPatchRule): string {
  if (rule.overridden_rule_ids.length > 0) {
    return t("modelEditPage.requestPatch.messages.overridesProviderRules", {
      ids: formatRuleIds(rule.overridden_rule_ids),
    });
  }

  return t("modelEditPage.requestPatch.messages.effectiveFromOrigin", {
    origin: t(`modelEditPage.requestPatch.origin.${rule.source_origin}`),
    id: `#${rule.source_rule_id}`,
  });
}

async function refreshExplainState(showLoading = false) {
  if (!props.modelId) {
    clearState();
    isLoading.value = false;
    loadError.value = null;
    return;
  }

  if (showLoading) {
    isLoading.value = true;
  } else {
    isRefreshing.value = true;
  }

  try {
    loadError.value = null;
    const [directResponse, explainResponse] = await Promise.all([
      Api.listModelRequestPatches(props.modelId),
      Api.getModelRequestPatchExplain(props.modelId),
    ]);
    directRules.value = directResponse;
    inheritedRules.value = explainResponse.inherited_rules;
    effectiveRules.value = explainResponse.effective_rules;
    explainEntries.value = explainResponse.explain;
    conflicts.value = explainResponse.conflicts;
    hasConflicts.value = explainResponse.has_conflicts;
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    if (showLoading) {
      loadError.value = normalizedError.message;
      clearState();
    } else {
      toastController.error(
        t("modelEditPage.requestPatch.alert.loadFailed"),
        normalizedError.message,
      );
    }
  } finally {
    isLoading.value = false;
    isRefreshing.value = false;
  }
}

function openCreateDialog() {
  if (!props.modelId) {
    toastController.warn(t("modelEditPage.requestPatch.alert.saveBeforeEdit"));
    return;
  }

  editorMode.value = "create";
  editorForm.value = createEmptyForm();
  isEditorOpen.value = true;
}

function openEditDialog(rule: RequestPatchRule) {
  editorMode.value = "edit";
  editorForm.value = {
    id: rule.id,
    placement: rule.placement,
    target: rule.target,
    operation: rule.operation,
    value_json_text:
      rule.operation === "REMOVE"
        ? ""
        : formatRequestPatchValueForEditor(rule.value_json),
    description: rule.description || "",
    is_enabled: rule.is_enabled,
  };
  isEditorOpen.value = true;
}

function handleEditorOpenChange(open: boolean) {
  isEditorOpen.value = open;
  if (!open && !isSubmitting.value) {
    editorForm.value = createEmptyForm();
  }
}

function handleDangerDialogOpenChange(open: boolean) {
  if (!open && !isConfirmingDangerous.value) {
    pendingDangerousConfirmation.value = null;
  }
  isDangerDialogOpen.value = open;
}

function cancelDangerousConfirmation() {
  pendingDangerousConfirmation.value = null;
  isDangerDialogOpen.value = false;
}

function buildPayload(
  confirmDangerousTarget = false,
): RequestPatchPayload | RequestPatchUpdatePayload {
  if (!editorForm.value.target.trim()) {
    throw new Error(t("modelEditPage.requestPatch.alert.targetRequired"));
  }
  if (
    editorForm.value.operation === "SET" &&
    !editorForm.value.value_json_text.trim()
  ) {
    throw new Error(t("modelEditPage.requestPatch.alert.valueRequired"));
  }

  return buildRequestPatchPayloadFromEditor(
    editorForm.value,
    confirmDangerousTarget,
  );
}

async function submitRule(
  payload: RequestPatchPayload | RequestPatchUpdatePayload,
  mode: EditorMode,
  ruleId: number | null,
) {
  if (!props.modelId) {
    toastController.warn(t("modelEditPage.requestPatch.alert.saveBeforeEdit"));
    return;
  }

  const outcome =
    mode === "create"
      ? await Api.createModelRequestPatch(props.modelId, payload as RequestPatchPayload)
      : await Api.updateModelRequestPatch(
          props.modelId,
          ruleId!,
          payload as RequestPatchUpdatePayload,
        );

  if (outcome.result === "confirmation_required") {
    pendingDangerousConfirmation.value = {
      mode,
      ruleId,
      payload,
      confirmation: outcome.confirmation,
    };
    isDangerDialogOpen.value = true;
    return;
  }

  await refreshExplainState(false);
  isEditorOpen.value = false;
  pendingDangerousConfirmation.value = null;
  isDangerDialogOpen.value = false;
  editorForm.value = createEmptyForm();
  toastController.success(
    mode === "create"
      ? t("modelEditPage.requestPatch.alert.createSuccess")
      : t("modelEditPage.requestPatch.alert.updateSuccess"),
  );
}

async function handleSaveRule() {
  if (!props.modelId) {
    toastController.warn(t("modelEditPage.requestPatch.alert.saveBeforeEdit"));
    return;
  }

  try {
    isSubmitting.value = true;
    const payload = buildPayload(false);
    await submitRule(payload, editorMode.value, editorForm.value.id);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.requestPatch.alert.saveFailed"),
      normalizedError.message,
    );
  } finally {
    isSubmitting.value = false;
  }
}

async function confirmDangerousSubmission() {
  const pending = pendingDangerousConfirmation.value;
  if (!pending) {
    return;
  }

  try {
    isConfirmingDangerous.value = true;
    const payload = Object.assign({}, pending.payload, {
      confirm_dangerous_target: true,
    }) as RequestPatchPayload | RequestPatchUpdatePayload;
    await submitRule(payload, pending.mode, pending.ruleId);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.requestPatch.alert.dangerousConfirmFailed"),
      normalizedError.message,
    );
  } finally {
    isConfirmingDangerous.value = false;
  }
}

async function handleToggleEnabled(rule: RequestPatchRule, nextValue: boolean) {
  if (!props.modelId) {
    toastController.warn(t("modelEditPage.requestPatch.alert.saveBeforeEdit"));
    return;
  }

  try {
    activeRuleId.value = rule.id;
    await Api.updateModelRequestPatch(props.modelId, rule.id, {
      is_enabled: nextValue,
    });
    await refreshExplainState(false);
    toastController.success(
      nextValue
        ? t("modelEditPage.requestPatch.alert.enableSuccess")
        : t("modelEditPage.requestPatch.alert.disableSuccess"),
    );
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.requestPatch.alert.toggleFailed"),
      normalizedError.message,
    );
  } finally {
    activeRuleId.value = null;
  }
}

async function handleDeleteRule(rule: RequestPatchRule) {
  if (!props.modelId) {
    toastController.warn(t("modelEditPage.requestPatch.alert.saveBeforeEdit"));
    return;
  }

  try {
    activeRuleId.value = rule.id;
    await Api.deleteModelRequestPatch(props.modelId, rule.id);
    await refreshExplainState(false);
    toastController.success(t("modelEditPage.requestPatch.alert.deleteSuccess"));
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.requestPatch.alert.deleteFailed"),
      normalizedError.message,
    );
  } finally {
    activeRuleId.value = null;
  }
}

function handleRefresh() {
  void refreshExplainState(false);
}

watch(
  () => props.modelId,
  () => {
    void refreshExplainState(true);
  },
  { immediate: true },
);
</script>
