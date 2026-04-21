<template>
  <div class="app-page flex h-full min-h-0 flex-col overflow-hidden">
    <div class="app-page-shell flex min-h-0 flex-1 flex-col">
      <div class="flex min-h-0 flex-1 flex-col gap-4 sm:gap-6">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
              {{ $t("recordPage.title") }}
            </h1>
            <p class="mt-1 text-sm text-gray-500">
              {{ $t("recordPage.description") || $t("recordPage.title") }}
            </p>
          </div>
        </div>

        <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
          <div class="flex flex-col gap-3 border-b border-gray-100 pb-4 md:flex-row md:items-start md:justify-between">
            <div class="min-w-0">
              <h2 class="text-base font-semibold text-gray-900">
                {{ $t("recordPage.filter.applyButton", "Filters") }}
              </h2>
              <p class="mt-1 text-sm text-gray-500">
                {{ mobileFilterSummary }}
              </p>
            </div>
            <div class="flex w-full flex-col gap-2 sm:flex-row md:w-auto md:items-center">
              <Button
                variant="outline"
                class="w-full justify-between md:hidden"
                @click="toggleFilterPanel"
              >
                <span class="flex items-center gap-2">
                  <SlidersHorizontal class="h-4 w-4" />
                  {{ isFilterPanelOpen ? "Hide filters" : "Show filters" }}
                </span>
                <ChevronDown
                  class="h-4 w-4 transition-transform"
                  :class="{ 'rotate-180': isFilterPanelOpen }"
                />
              </Button>
              <Button
                v-if="hasActiveFilters"
                variant="outline"
                class="hidden md:inline-flex"
                @click="handleResetFilter"
              >
                {{ $t("recordPage.filter.resetButton") }}
              </Button>
            </div>
          </div>

          <div
            :class="[
              'mt-4 flex-col gap-4 md:mt-4 md:flex',
              isFilterPanelOpen ? 'flex' : 'hidden md:flex',
            ]"
          >
            <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-12">
              <div class="flex flex-col gap-1.5 xl:col-span-3">
                <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.table.apiKey") }}
                </span>
                <Select
                  :model-value="String(filters.api_key_id)"
                  @update:model-value="handleApiKeyFilterChange"
                >
                  <SelectTrigger class="w-full">
                    <SelectValue :placeholder="$t('recordPage.filter.allApiKeys')" />
                  </SelectTrigger>
                  <SelectContent :body-lock="false">
                    <SelectItem
                      v-for="opt in apiKeyOptions"
                      :key="opt.value"
                      :value="String(opt.value)"
                    >
                      {{ opt.label }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div class="flex flex-col gap-1.5 xl:col-span-3">
                <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.table.provider") }}
                </span>
                <Select
                  :model-value="String(filters.provider_id)"
                  @update:model-value="handleProviderFilterChange"
                >
                  <SelectTrigger class="w-full">
                    <SelectValue :placeholder="$t('recordPage.filter.allProviders')" />
                  </SelectTrigger>
                  <SelectContent :body-lock="false">
                    <SelectItem
                      v-for="opt in providerOptions"
                      :key="opt.value"
                      :value="String(opt.value)"
                    >
                      {{ opt.label }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div class="flex flex-col gap-1.5 xl:col-span-2">
                <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.table.status") }}
                </span>
                <Select
                  :model-value="filters.status"
                  @update:model-value="handleStatusFilterChange"
                >
                  <SelectTrigger class="w-full">
                    <SelectValue :placeholder="$t('recordPage.filter.allStatuses')" />
                  </SelectTrigger>
                  <SelectContent :body-lock="false">
                    <SelectItem
                      v-for="opt in statusOptions"
                      :key="opt.value"
                      :value="opt.value"
                    >
                      {{ opt.label }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div class="flex flex-col gap-1.5 xl:col-span-4">
                <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.filter.searchPlaceholder", "Search") }}
                </span>
                <div class="relative">
                  <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
                  <Input
                    v-model="searchInput"
                    :placeholder="$t('recordPage.filter.searchPlaceholder')"
                    class="w-full pl-9 pr-9"
                    @keydown.enter="handleApplyFilter"
                  />
                  <button
                    v-if="searchInput"
                    type="button"
                    class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 transition-colors hover:text-gray-600"
                    @click="handleClearSearch"
                  >
                    <X class="h-4 w-4" />
                  </button>
                </div>
              </div>
            </div>

            <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <p class="text-xs text-gray-500">
                {{
                  hasActiveFilters
                    ? mobileFilterSummary
                    : "Choose filters or search terms to narrow the records list."
                }}
              </p>
              <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
                <Button class="w-full sm:w-auto" @click="handleApplyFilter">
                  {{ $t("recordPage.filter.applyButton") }}
                </Button>
                <Button
                  v-if="hasActiveFilters"
                  variant="outline"
                  class="w-full md:hidden sm:w-auto"
                  @click="handleResetFilter"
                >
                  {{ $t("recordPage.filter.resetButton") }}
                </Button>
              </div>
            </div>
          </div>
        </div>

        <div v-if="isLoading" class="py-10 text-center text-gray-500">
          <div
            class="mb-2 inline-block h-8 w-8 animate-spin rounded-full border-b-2 border-gray-900"
          ></div>
          <div>{{ $t("recordPage.loading") }}</div>
        </div>

        <div
          v-else-if="errorMsg"
          class="rounded-lg border border-red-400 bg-red-100 p-4 py-4 text-center text-red-600"
        >
          {{ $t("recordPage.errorPrefix") }} {{ errorMsg }}
        </div>

        <div
          v-else
          class="flex min-h-0 flex-1 flex-col rounded-xl border border-gray-200 bg-white"
        >
          <div v-if="records.length === 0" class="px-4 py-10 text-center text-sm text-gray-500">
            {{
              totalRecords === 0
                ? $t("recordPage.table.noRecordsMatch")
                : $t("recordPage.table.noRecordsAvailable")
            }}
          </div>

          <div v-else class="space-y-3 p-3 md:hidden">
            <MobileCrudCard
              v-for="record in records"
              :key="record.id"
              :title="record.displayRequestedModelName"
              :description="record.request_at_formatted"
            >
              <div class="grid grid-cols-1 gap-3 text-sm min-[360px]:grid-cols-2">
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.provider") }}
                  </p>
                  <p class="break-words text-sm text-gray-900">
                    {{ record.providerName }}
                  </p>
                </div>
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.apiKey") }}
                  </p>
                  <p class="break-words text-sm text-gray-900">
                    {{ record.apiKeyName }}
                  </p>
                </div>
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.status") }}
                  </p>
                  <div>
                    <Badge :variant="getStatusBadgeVariant(record.status)">
                      {{ record.status || $t("common.notAvailable") }}
                    </Badge>
                  </div>
                </div>
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.stream") }}
                  </p>
                  <p class="text-sm text-gray-900">{{ record.isStreamDisplay }}</p>
                </div>
              </div>

              <div class="grid grid-cols-1 gap-3 rounded-lg bg-gray-50 p-3 min-[360px]:grid-cols-2">
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.totalTokens") }}
                  </p>
                  <p class="text-sm font-semibold text-gray-900">
                    {{ record.total_tokens ?? "/" }}
                  </p>
                </div>
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.cost") }}
                  </p>
                  <p class="break-all font-mono text-xs text-gray-700">
                    {{ record.costDisplay }}
                  </p>
                </div>
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.firstResp") }}
                  </p>
                  <p class="text-sm text-gray-900">{{ record.firstRespTimeDisplay }}</p>
                </div>
                <div class="space-y-1">
                  <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.table.tps") }}
                  </p>
                  <p class="text-sm text-gray-900">{{ record.tpsDisplay }}</p>
                </div>
              </div>

              <template #actions>
                <Button class="w-full" @click="handleViewDetails(record.id)">
                  {{ $t("recordPage.table.viewDetails") }}
                </Button>
              </template>
            </MobileCrudCard>
          </div>

          <div class="hidden flex-1 overflow-auto md:block">
            <Table>
              <TableHeader class="bg-gray-50/80 hover:bg-gray-50/80">
                <TableRow>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.modelName") }}
                  </TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.provider") }}
                  </TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.apiKey") }}
                  </TableHead>
                  <TableHead class="w-14 text-center text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.status") }}
                  </TableHead>
                  <TableHead class="min-w-[220px] text-xs font-medium uppercase tracking-wider text-gray-500">
                    Tokens
                  </TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.stream") }}
                  </TableHead>
                  <TableHead class="min-w-[200px] text-xs font-medium uppercase tracking-wider text-gray-500">
                    Performance
                  </TableHead>
                  <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.cost") }}
                  </TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.requestTime") }}
                  </TableHead>
                  <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("recordPage.table.details") }}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="record in records"
                  :key="record.id"
                  class="hover:bg-gray-50"
                >
                  <TableCell class="font-medium">{{ record.displayRequestedModelName }}</TableCell>
                  <TableCell>{{ record.providerName }}</TableCell>
                  <TableCell>{{ record.apiKeyName }}</TableCell>
                  <TableCell class="w-14 text-center">
                    <div
                      class="flex justify-center"
                      :title="getStatusMeta(record.status).label"
                      :aria-label="getStatusMeta(record.status).label"
                    >
                      <component
                        :is="getStatusMeta(record.status).icon"
                        class="h-4 w-4"
                        :class="getStatusMeta(record.status).className"
                      />
                      <span class="sr-only">{{ getStatusMeta(record.status).label }}</span>
                    </div>
                  </TableCell>
                  <TableCell
                    class="font-mono text-xs text-gray-700"
                    :title="`${$t('recordPage.table.promptTokens')} / ${$t('recordPage.table.completionTokens')} / ${$t('recordPage.table.reasoningTokens')} / ${$t('recordPage.table.totalTokens')}`"
                  >
                    {{ formatCompactMetrics([
                      record.total_input_tokens,
                      record.total_output_tokens,
                      record.reasoning_tokens,
                      record.total_tokens,
                    ]) }}
                  </TableCell>
                  <TableCell>{{ record.isStreamDisplay }}</TableCell>
                  <TableCell
                    class="font-mono text-xs text-gray-700"
                    :title="`${$t('recordPage.table.firstResp')} / ${$t('recordPage.table.totalResp')} / ${$t('recordPage.table.tps')}`"
                  >
                    {{ formatCompactMetrics([
                      record.firstRespTimeDisplay,
                      record.totalRespTimeDisplay,
                      record.tpsDisplay,
                    ]) }}
                  </TableCell>
                  <TableCell class="text-right font-mono text-gray-600">{{ record.costDisplay }}</TableCell>
                  <TableCell class="whitespace-nowrap text-sm text-gray-500">
                    {{ record.request_at_formatted }}
                  </TableCell>
                  <TableCell class="text-right">
                    <Button
                      variant="link"
                      size="sm"
                      class="px-0"
                      @click="handleViewDetails(record.id)"
                    >
                      {{ $t("recordPage.table.viewDetails") }}
                    </Button>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>

          <div
            v-if="totalPages > 0"
            class="mt-auto flex flex-shrink-0 flex-col gap-4 border-t border-gray-100 px-4 py-4 sm:px-5 md:flex-row md:items-center md:justify-between"
          >
            <div
              class="order-2 flex flex-col gap-3 text-sm text-gray-500 md:order-1 md:flex-row md:items-center md:gap-4"
            >
              <div>
                {{ $t("recordPage.pagination.page") }}
                <span class="font-medium text-gray-900">{{ currentPage }}</span>
                {{ $t("recordPage.pagination.of") }}
                <span class="font-medium text-gray-900">{{ totalPages }}</span>
                (<span class="font-medium text-gray-900">{{ totalRecords }}</span>
                {{ $t("recordPage.pagination.items") }})
              </div>
              <div class="flex items-center justify-between gap-2 sm:justify-start">
                <label class="whitespace-nowrap">{{
                  $t("recordPage.pagination.itemsPerPage")
                }}</label>
                <Select
                  :model-value="String(pageSize)"
                  @update:model-value="handlePageSizeChange"
                >
                  <SelectTrigger class="h-8 w-[70px] text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem
                      v-for="size in [10, 25, 50, 100]"
                      :key="size"
                      :value="String(size)"
                      class="text-xs"
                    >
                      {{ size }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>

            <Pagination
              v-if="totalPages > 1"
              :total="totalRecords"
              :sibling-count="1"
              :items-per-page="pageSize"
              :page="currentPage"
              show-edges
              class="order-1 mx-0 w-full md:order-2 md:w-auto"
              @update:page="handlePageChange"
            >
              <PaginationContent
                v-slot="{ items }"
                class="flex flex-wrap items-center justify-center gap-1 md:justify-end"
              >
                <PaginationFirst />
                <PaginationPrevious />
                <template v-for="(item, index) in items" :key="index">
                  <PaginationItem
                    v-if="item.type === 'page'"
                    :value="item.value"
                    :is-active="item.value === currentPage"
                  >
                    {{ item.value }}
                  </PaginationItem>
                  <PaginationEllipsis v-else />
                </template>
                <PaginationNext />
                <PaginationLast />
              </PaginationContent>
            </Pagination>
          </div>
        </div>
      </div>
    </div>

    <Dialog v-model:open="isDetailModalOpen">
      <DialogContent
        class="flex max-h-[92dvh] flex-col p-0 sm:max-w-[85vw] md:max-w-6xl"
      >
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="pr-8 text-base font-semibold text-gray-900 sm:text-lg">
            {{ $t("recordPage.detailModal.title", "Log Details") }}
          </DialogTitle>
        </DialogHeader>

        <div class="flex-grow overflow-y-auto px-4 py-4 sm:px-6 sm:pt-3">
          <div v-if="isDetailLoading" class="py-10 text-center">
            <div
              class="mb-2 inline-block h-8 w-8 animate-spin rounded-full border-b-2 border-gray-900"
            ></div>
            <div>{{ $t("recordPage.loading") }}</div>
          </div>
          <div v-else-if="detailedRecord" class="space-y-4 text-sm sm:space-y-5">
            <section class="border-b border-gray-100 pb-1">
              <dl class="grid grid-cols-1 divide-y divide-gray-100 sm:grid-cols-2 sm:divide-x sm:divide-y-0 xl:grid-cols-4">
                <div class="flex items-center justify-between gap-3 px-4 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">Status</dt>
                  <dd>
                    <Badge :variant="getStatusBadgeVariant(detailedRecord.status)">
                      {{ detailedRecord.status || $t("common.notAvailable") }}
                    </Badge>
                  </dd>
                </div>
                <div class="flex items-center justify-between gap-3 px-4 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">Provider</dt>
                  <dd class="truncate text-right text-sm font-medium text-gray-900">
                    {{ getProviderName(detailedRecord.provider_id) }}
                  </dd>
                </div>
                <div class="flex items-center justify-between gap-3 px-4 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">Model</dt>
                  <dd class="truncate text-right font-mono text-xs text-gray-900 sm:text-sm">
                    {{ detailedRecord.requested_model_name || detailedRecord.model_name || "/" }}
                  </dd>
                </div>
                <div class="flex items-center justify-between gap-3 px-4 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.detailModal.totalTokens") }}
                  </dt>
                  <dd class="text-right text-sm font-semibold text-gray-900 sm:text-base">
                    {{ detailedRecord.total_tokens ?? "/" }}
                  </dd>
                </div>
              </dl>
            </section>

            <div class="space-y-4">
              <section class="pt-1">
                  <h3 class="mb-3 border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
                    {{ $t("recordPage.detailModal.general") }}
                  </h3>
                  <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2">
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">ID</dt>
                      <dd class="font-mono text-xs text-gray-900 sm:text-sm">{{ detailedRecord.id }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">API Key</dt>
                      <dd class="truncate text-right text-sm text-gray-900">{{ getApiKeyName(detailedRecord.api_key_id) }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Requested Model</dt>
                      <dd class="truncate text-right font-mono text-xs text-gray-900 sm:text-sm">
                        {{ detailedRecord.requested_model_name || detailedRecord.model_name || "/" }}
                      </dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Resolved Scope</dt>
                      <dd class="truncate text-right text-sm text-gray-900">
                        {{ formatResolvedScope(detailedRecord.resolved_name_scope) }}
                      </dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Resolved Route</dt>
                      <dd class="truncate text-right font-mono text-xs text-gray-900 sm:text-sm">
                        {{ detailedRecord.resolved_route_name || "/" }}
                      </dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Selected Model</dt>
                      <dd class="truncate text-right font-mono text-xs text-gray-900 sm:text-sm">
                        {{ detailedRecord.model_name || "/" }}
                      </dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Real Model</dt>
                      <dd class="truncate text-right font-mono text-xs text-gray-900 sm:text-sm">{{ detailedRecord.real_model_name || "/" }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Stream</dt>
                      <dd class="text-right text-sm text-gray-900">{{ detailedRecord.is_stream ? $t("common.yes") : $t("common.no") }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">User API Type</dt>
                      <dd class="truncate text-right text-sm text-gray-900">{{ detailedRecord.user_api_type || $t("common.notAvailable") }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">LLM API Type</dt>
                      <dd class="truncate text-right text-sm text-gray-900">{{ detailedRecord.llm_api_type || $t("common.notAvailable") }}</dd>
                    </div>
                  </dl>
              </section>

              <section class="border-t border-gray-100 pt-4">
                  <h3 class="mb-3 border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
                    {{ $t("recordPage.detailModal.usageSummary") }}
                  </h3>
                  <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-4">
                    <div
                      v-for="item in visibleUsageSummaryItems"
                      :key="item.label"
                      class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5 last:border-b-0"
                    >
                      <dt class="text-xs text-gray-500">{{ item.label }}</dt>
                      <dd class="font-semibold text-gray-900">{{ item.value }}</dd>
                    </div>
                  </dl>
              </section>

              <section class="border-t border-gray-100 pt-4">
                  <h3 class="mb-3 border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
                    {{ $t("recordPage.detailModal.timings") }}
                  </h3>
                  <dl class="grid grid-cols-1 gap-x-6 md:grid-cols-2">
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Request Received</dt>
                      <dd class="text-right text-sm text-gray-900">{{ formatDate(detailedRecord.request_received_at) }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">LLM Request Sent</dt>
                      <dd class="text-right text-sm text-gray-900">{{ formatDate(detailedRecord.llm_request_sent_at) }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">LLM First Chunk</dt>
                      <dd class="text-right text-sm text-gray-900">{{ formatDate(detailedRecord.llm_response_first_chunk_at) }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">LLM Completed</dt>
                      <dd class="text-right text-sm text-gray-900">{{ formatDate(detailedRecord.llm_response_completed_at) }}</dd>
                    </div>
                    <div class="flex items-center justify-between gap-3 py-2.5">
                      <dt class="text-xs uppercase tracking-wide text-gray-500">Response to Client</dt>
                      <dd class="text-right text-sm text-gray-900">{{ formatDate(detailedRecord.response_sent_to_client_at) }}</dd>
                    </div>
                  </dl>
              </section>

              <section class="border-t border-gray-100 pt-4">
              <h3 class="mb-3 border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
                {{ $t("recordPage.detailModal.costSnapshot") }}
              </h3>
              <div v-if="parsedCostSnapshot" class="space-y-4">
                <div class="flex items-center justify-between py-1">
                  <div>
                    <div class="text-xs text-gray-500">
                      {{ $t("recordPage.detailModal.totalCost") }}
                    </div>
                    <div class="mt-1 text-lg font-semibold text-gray-900">
                      {{ formatPriceFromNanos(parsedCostSnapshot.total_cost_nanos, parsedCostSnapshot.currency, "/") }}
                    </div>
                  </div>
                  <Popover v-if="costSnapshotIssues.length > 0">
                    <PopoverTrigger as-child>
                      <button
                        type="button"
                        class="inline-flex h-8 w-8 items-center justify-center rounded-full border border-amber-200 bg-amber-50 text-amber-700 transition hover:bg-amber-100"
                      >
                        <CircleAlert class="h-4 w-4" />
                      </button>
                    </PopoverTrigger>
                    <PopoverContent
                      align="end"
                      class="w-80 border-amber-200 bg-white p-3 text-sm text-gray-700"
                    >
                      <div class="mb-2 font-medium text-amber-900">
                        {{ $t("recordPage.detailModal.warnings") }}
                      </div>
                      <ul class="space-y-2">
                        <li
                          v-for="(issue, issueIndex) in costSnapshotIssues"
                          :key="`${issue.label}-${issue.value}-${issueIndex}`"
                          class="rounded-md bg-amber-50 px-3 py-2 text-amber-900"
                        >
                          <span class="font-medium">{{ issue.label }}:</span>
                          {{ issue.value }}
                        </li>
                      </ul>
                    </PopoverContent>
                  </Popover>
                </div>

                <div class="space-y-2">
                  <h4 class="text-sm font-semibold text-gray-900">
                    {{ $t("recordPage.detailModal.detailLines") }}
                  </h4>
                  <div
                    v-if="parsedCostSnapshot.detail_lines.length === 0"
                    class="px-1 py-4 text-sm text-gray-500"
                  >
                    {{ $t("recordPage.detailModal.noDetailLines") }}
                  </div>
                  <div v-else class="overflow-hidden border-t border-b border-gray-100">
                    <div
                      class="hidden grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)_auto] gap-3 border-b border-gray-100 bg-gray-50 px-4 py-2 text-[11px] font-medium uppercase tracking-wide text-gray-500 md:grid"
                    >
                      <div>{{ $t("recordPage.detailModal.billingItem") }}</div>
                      <div>{{ $t("recordPage.detailModal.quantity") }}</div>
                      <div>{{ $t("recordPage.detailModal.unitPrice") }}</div>
                      <div class="text-right">{{ $t("recordPage.detailModal.finalPrice") }}</div>
                    </div>
                    <div
                      v-for="(line, index) in displayedCostDetailLines"
                      :key="`${line.component_id ?? 'none'}-${index}`"
                      class="grid grid-cols-1 gap-2 border-t border-gray-100 px-4 py-3 first:border-t-0 md:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)_auto]"
                    >
                      <div class="flex min-w-0 items-center gap-2">
                        <Badge variant="outline" class="max-w-full font-mono text-[11px]">
                          {{ line.meter_key }}
                        </Badge>
                        <Popover v-if="line.extraInfoLines.length > 0">
                          <PopoverTrigger as-child>
                            <button
                              type="button"
                              class="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-gray-400 transition hover:bg-gray-100 hover:text-gray-700"
                            >
                              <CircleHelp class="h-4 w-4" />
                            </button>
                          </PopoverTrigger>
                          <PopoverContent
                            align="start"
                            class="w-80 border-gray-200 bg-white p-3 text-sm text-gray-700"
                          >
                            <div class="space-y-2">
                              <div
                                v-for="(info, infoIndex) in line.extraInfoLines"
                                :key="`${line.meter_key}-${infoIndex}`"
                                class="rounded-md bg-gray-50 px-3 py-2"
                              >
                                {{ info }}
                              </div>
                            </div>
                          </PopoverContent>
                        </Popover>
                      </div>
                      <div class="text-sm text-gray-700">
                        <span class="mr-2 text-[11px] uppercase tracking-wide text-gray-400 md:hidden">
                          {{ $t("recordPage.detailModal.quantity") }}
                        </span>
                        {{ line.quantity }} {{ line.unit }}
                      </div>
                      <div class="text-sm text-gray-700">
                        <span class="mr-2 text-[11px] uppercase tracking-wide text-gray-400 md:hidden">
                          {{ $t("recordPage.detailModal.unitPrice") }}
                        </span>
                        <span v-if="line.unit_price_nanos !== null">
                          {{
                            formatSnapshotUnitPrice(
                              line.meter_key,
                              line.unit_price_nanos,
                              parsedCostSnapshot.currency,
                            )
                          }}
                        </span>
                        <span v-else>/</span>
                      </div>
                      <div class="text-sm font-semibold text-gray-900 md:text-right">
                        <span class="mr-2 text-[11px] uppercase tracking-wide text-gray-400 md:hidden">
                          {{ $t("recordPage.detailModal.finalPrice") }}
                        </span>
                        {{ formatPriceFromNanos(line.amount_nanos, parsedCostSnapshot.currency, "/") }}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
              <div
                v-else-if="detailedRecord.cost_snapshot_json"
                class="space-y-2"
              >
                <p class="text-sm text-amber-700">
                  {{ $t("recordPage.detailModal.invalidCostSnapshot") }}
                </p>
                <pre class="overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100">{{ detailedRecord.cost_snapshot_json }}</pre>
              </div>
              <div
                v-else
                class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
              >
                {{ $t("recordPage.detailModal.noCostSnapshot") }}
              </div>
              </section>
            </div>

            <section class="border-t border-gray-100 pt-4">
              <h3 class="mb-3 border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
                {{ $t("recordPage.detailModal.requestPatch") }}
              </h3>

              <div v-if="hasInvalidRequestPatchTrace" class="space-y-2">
                <p class="text-sm text-amber-700">
                  {{ $t("recordPage.detailModal.invalidRequestPatchTrace") }}
                </p>
                <div v-if="detailedRecord.request_patch_summary_json" class="space-y-2">
                  <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("recordPage.detailModal.rawTraceSummary") }}
                  </p>
                  <pre class="overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100">{{ detailedRecord.request_patch_summary_json }}</pre>
                </div>
              </div>

              <div v-else-if="parsedRequestPatchTrace" class="space-y-4">
                <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-4 py-3">
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ $t("recordPage.detailModal.appliedRuleIds") }}
                    </p>
                    <p class="mt-1 break-all font-mono text-sm text-gray-900">
                      {{ formattedAppliedRequestPatchIds || "/" }}
                    </p>
                  </div>

                  <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-4 py-3">
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ $t("recordPage.detailModal.conflicts") }}
                    </p>
                    <p class="mt-1 text-sm font-medium text-gray-900">
                      {{ requestPatchConflicts.length }}
                    </p>
                  </div>
                </div>

                <div
                  v-if="parsedRequestPatchTrace.has_conflicts && requestPatchConflicts.length > 0"
                  class="rounded-lg border border-red-200 bg-red-50 px-4 py-3"
                >
                  <p class="text-sm font-medium text-red-700">
                    {{ $t("recordPage.detailModal.conflictDetected") }}
                  </p>

                  <div class="mt-3 space-y-3">
                    <div
                      v-for="conflict in requestPatchConflicts"
                      :key="`${conflict.provider_rule_id}-${conflict.model_rule_id}-${conflict.placement}`"
                      class="rounded-md border border-red-200 bg-white/70 px-3 py-2.5"
                    >
                      <p class="text-sm text-red-700">
                        {{ conflict.reason }}
                      </p>
                      <p class="mt-1 break-all font-mono text-xs text-red-600">
                        #{{ conflict.provider_rule_id }} · #{{ conflict.model_rule_id }} ·
                        {{ conflict.placement }} · {{ conflict.provider_target }} /
                        {{ conflict.model_target }}
                      </p>
                    </div>
                  </div>
                </div>

                <div class="space-y-3">
                  <div class="min-w-0">
                    <h4 class="text-sm font-medium text-gray-900">
                      {{ $t("recordPage.detailModal.effectiveRules") }}
                    </h4>
                  </div>

                  <div
                    v-if="requestPatchEffectiveRules.length === 0"
                    class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
                  >
                    {{ $t("recordPage.detailModal.noEffectiveRules") }}
                  </div>

                  <div
                    v-else
                    class="overflow-hidden rounded-lg border border-gray-200 bg-white"
                  >
                    <div
                      v-for="rule in requestPatchEffectiveRules"
                      :key="`${rule.source_rule_id}-${rule.placement}-${rule.target}`"
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
                          <Badge
                            class="text-[11px]"
                            :variant="rule.source_origin === 'ModelDirect' ? 'default' : 'secondary'"
                          >
                            {{ getRequestPatchOriginLabel(rule.source_origin) }}
                          </Badge>
                        </div>

                        <p class="break-all font-mono text-sm text-gray-900">
                          {{ rule.target }}
                        </p>

                        <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
                          <div>
                            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                              {{ $t("recordPage.detailModal.value") }}
                            </p>
                            <p class="mt-1 break-all font-mono text-sm text-gray-700">
                              {{ formatRequestPatchValueForDisplay(rule.value_json) }}
                            </p>
                          </div>

                          <div>
                            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                              {{ $t("recordPage.detailModal.description") }}
                            </p>
                            <p class="mt-1 text-sm text-gray-600">
                              {{
                                rule.description ||
                                $t("recordPage.detailModal.noRuleDescription")
                              }}
                            </p>
                          </div>

                          <div>
                            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                              {{ $t("recordPage.detailModal.sourceRule") }}
                            </p>
                            <p class="mt-1 font-mono text-xs text-gray-600">
                              #{{ rule.source_rule_id }}
                            </p>
                          </div>

                          <div>
                            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                              {{ $t("recordPage.detailModal.trace") }}
                            </p>
                            <p class="mt-1 text-sm text-gray-600">
                              {{ getRequestPatchTrace(rule) }}
                            </p>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <div
                v-else
                class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
              >
                {{ $t("recordPage.detailModal.noRequestPatchTrace") }}
              </div>
            </section>

            <section class="border-t border-gray-100 pt-4">
              <h3 class="mb-3 border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
                Payloads
              </h3>
              <div v-if="showPayloads" class="space-y-4">
                <template v-if="detailedRecord.storage_type">
                  <BodyViewer
                    :record-id="detailedRecord.id"
                    :storage-type="detailedRecord.storage_type"
                    :status="detailedRecord.status"
                  />
                </template>
                <template v-else>
                  <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                    <SingleRequestBodyContent
                      :content="detailedRecord.user_request_body ?? null"
                      title="User Request Body"
                    />
                    <SingleRequestBodyContent
                      :content="detailedRecord.llm_request_body ?? null"
                      title="LLM Request Body"
                    />
                    <SingleResponseBodyContent
                      :content="detailedRecord.llm_response_body ?? null"
                      title="LLM Response Body"
                      :status="detailedRecord.status"
                    />
                    <SingleResponseBodyContent
                      :content="detailedRecord.user_response_body ?? null"
                      title="User Response Body"
                      :status="detailedRecord.status"
                    />
                  </div>
                </template>
              </div>
              <p v-else class="text-sm text-gray-500">
                Rendering request and response payloads...
              </p>
            </section>
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6 sm:pt-3">
          <Button
            variant="secondary"
            class="w-full sm:w-auto"
            @click="isDetailModalOpen = false"
          >
            {{ $t("common.close", "Close") }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  reactive,
  ref,
  watch,
} from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter, type LocationQuery } from "vue-router";
import {
  CircleAlert,
  CircleCheckBig,
  CircleHelp,
  ChevronDown,
  Clock3,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-vue-next";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import BodyViewer from "@/components/record/BodyViewer.vue";
import SingleRequestBodyContent from "@/components/record/SingleRequestBodyContent.vue";
import SingleResponseBodyContent from "@/components/record/SingleResponseBodyContent.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationFirst,
  PaginationItem,
  PaginationLast,
  PaginationNext,
  PaginationPrevious,
} from "@/components/ui/pagination";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { normalizeError } from "@/lib/error";
import { formatRequestPatchValueForDisplay } from "@/lib/requestPatch";
import { toastController } from "@/lib/toastController";
import {
  formatCostRateFromNanos,
  formatPriceFromNanos,
  formatTimestamp,
} from "@/lib/utils";
import { Api } from "@/services/request";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { useProviderStore } from "@/store/providerStore";
import type {
  CostDetailLine,
  CostSnapshot,
  RecordDetail,
  RecordListItem,
  RequestPatchTraceSummary,
  ResolvedRequestPatchRule,
} from "@/store/types";

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();
const providerStore = useProviderStore();
const apiKeyStore = useApiKeyStore();

const DEFAULT_PAGE = 1;
const FALLBACK_PAGE_SIZE = 10;
const getStoredPageSize = () =>
  Number(localStorage.getItem("pageSize")) || FALLBACK_PAGE_SIZE;

type RecordFilters = {
  api_key_id: number;
  provider_id: number;
  status: string;
  search: string;
};

const DEFAULT_FILTERS: RecordFilters = {
  api_key_id: 0,
  provider_id: 0,
  status: "ALL",
  search: "",
};

const VALID_STATUSES = new Set(["ALL", "SUCCESS", "PENDING", "ERROR"]);

const records = ref<
  (RecordListItem & {
    providerName: string;
    apiKeyName: string;
    displayRequestedModelName: string;
    resolvedScopeDisplay: string;
    isStreamDisplay: string;
    firstRespTimeDisplay: string;
    totalRespTimeDisplay: string;
    tpsDisplay: string;
    costDisplay: string;
    request_at_formatted: string;
  })[]
>([]);
const totalRecords = ref(0);
const isLoading = ref(false);
const errorMsg = ref<string | null>(null);
const currentPage = ref(DEFAULT_PAGE);
const pageSize = ref(getStoredPageSize());
const searchInput = ref("");
const isFilterPanelOpen = ref(false);

const filters = reactive<RecordFilters>({
  api_key_id: DEFAULT_FILTERS.api_key_id,
  provider_id: DEFAULT_FILTERS.provider_id,
  status: DEFAULT_FILTERS.status,
  search: DEFAULT_FILTERS.search,
});

const isDetailModalOpen = ref(false);
const isDetailLoading = ref(false);
const detailedRecord = ref<RecordDetail | null>(null);
const showPayloads = ref(false);

const parsedCostSnapshot = computed<CostSnapshot | null>(() => {
  const raw = detailedRecord.value?.cost_snapshot_json;
  if (!raw) {
    return null;
  }

  try {
    return JSON.parse(raw) as CostSnapshot;
  } catch {
    return null;
  }
});

const parsedRequestPatchTrace = computed<RequestPatchTraceSummary | null>(() => {
  const raw = detailedRecord.value?.request_patch_summary_json;
  if (!raw) {
    return null;
  }

  try {
    return JSON.parse(raw) as RequestPatchTraceSummary;
  } catch {
    return null;
  }
});

const parsedAppliedRequestPatchIds = computed<number[]>(() => {
  const raw = detailedRecord.value?.applied_request_patch_ids_json;
  if (!raw) {
    return [];
  }

  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed.filter((id): id is number => typeof id === "number");
  } catch {
    return [];
  }
});

const requestPatchExplainByRuleId = computed(() => {
  return new Map(
    (parsedRequestPatchTrace.value?.explain ?? []).map((entry) => [
      entry.rule.id,
      entry,
    ]),
  );
});

const requestPatchEffectiveRules = computed(() => {
  return parsedRequestPatchTrace.value?.effective_rules ?? [];
});

const requestPatchConflicts = computed(() => {
  return parsedRequestPatchTrace.value?.conflicts ?? [];
});

const hasInvalidRequestPatchTrace = computed(() => {
  return Boolean(
    detailedRecord.value?.request_patch_summary_json &&
      parsedRequestPatchTrace.value === null,
  );
});

const formattedAppliedRequestPatchIds = computed(() => {
  if (parsedAppliedRequestPatchIds.value.length === 0) {
    return null;
  }

  return parsedAppliedRequestPatchIds.value
    .map((id) => `#${id}`)
    .join(", ");
});

type DisplayCostDetailLine = CostDetailLine & {
  extraInfoLines: string[];
};

const displayedCostDetailLines = computed<DisplayCostDetailLine[]>(() => {
  return (parsedCostSnapshot.value?.detail_lines ?? []).map((line) => {
    const attributes = { ...(line.attributes ?? {}) };
    const fallbackMeterKey = attributes.fallback_meter_key ?? null;
    delete attributes.fallback_meter_key;
    const extraInfo = [
      line.description
        ? `${$t("recordPage.detailModal.noDescription")}: ${line.description}`
        : null,
      line.component_id !== null
        ? `${$t("recordPage.detailModal.componentId")}: ${line.component_id}`
        : null,
      line.catalog_version_id !== null
        ? `${$t("recordPage.detailModal.versionId")}: ${line.catalog_version_id}`
        : null,
      fallbackMeterKey
        ? `${$t("recordPage.detailModal.fallbackPricingApplied")}: ${fallbackMeterKey}`
        : null,
      ...Object.entries(attributes).map(
        ([attributeKey, attributeValue]) => `${attributeKey}: ${attributeValue}`,
      ),
    ].filter((value): value is string => value !== null);

    return {
      ...line,
      extraInfoLines: extraInfo,
    };
  });
});

const visibleUsageSummaryItems = computed(() => {
  const record = detailedRecord.value;
  if (!record) {
    return [];
  }

  return [
    { label: $t("recordPage.detailModal.totalInputTokens"), value: record.total_input_tokens },
    { label: $t("recordPage.detailModal.totalOutputTokens"), value: record.total_output_tokens },
    { label: $t("recordPage.detailModal.totalTokens"), value: record.total_tokens },
    { label: $t("recordPage.detailModal.inputTextTokens"), value: record.input_text_tokens },
    { label: $t("recordPage.detailModal.outputTextTokens"), value: record.output_text_tokens },
    { label: $t("recordPage.detailModal.reasoningTokens"), value: record.reasoning_tokens },
    { label: $t("recordPage.detailModal.inputImageTokens"), value: record.input_image_tokens },
    { label: $t("recordPage.detailModal.outputImageTokens"), value: record.output_image_tokens },
    { label: $t("recordPage.detailModal.cacheReadTokens"), value: record.cache_read_tokens },
    { label: $t("recordPage.detailModal.cacheWriteTokens"), value: record.cache_write_tokens },
  ].filter((item) => item.value != null);
});

const costSnapshotIssues = computed(() => {
  return [
    ...(parsedCostSnapshot.value?.warnings ?? []).map(
      (warning) => ({
        label: $t("recordPage.detailModal.warnings"),
        value: warning,
      }),
    ),
    ...(parsedCostSnapshot.value?.unmatched_items ?? []).map(
      (item) => ({
        label: $t("recordPage.detailModal.unmatchedItems"),
        value: item,
      }),
    ),
  ];
});

const formatSnapshotUnitPrice = (
  meterKey: string,
  unitPriceNanos: number | null,
  currency?: string | null,
) => {
  const mode = meterKey.startsWith("llm.") ? "per_million_units" : "money";
  const base = formatCostRateFromNanos(unitPriceNanos, mode, currency, "/");
  return meterKey.startsWith("llm.") ? `${base} tokens` : `${base}/unit`;
};

function getRequestPatchOriginLabel(origin: string): string {
  return origin === "ModelDirect"
    ? $t("recordPage.detailModal.originModelDirect")
    : $t("recordPage.detailModal.originProviderDirect");
}

function formatRuleIds(ids: number[]): string {
  return ids.map((id) => `#${id}`).join(", ");
}

function getRequestPatchTrace(rule: ResolvedRequestPatchRule): string {
  const explainEntry = requestPatchExplainByRuleId.value.get(rule.source_rule_id);
  if (explainEntry?.message) {
    return explainEntry.message;
  }

  const baseTrace = `${getRequestPatchOriginLabel(rule.source_origin)} #${rule.source_rule_id}`;
  if (rule.overridden_rule_ids.length === 0) {
    return baseTrace;
  }

  return `${baseTrace} -> ${formatRuleIds(rule.overridden_rule_ids)}`;
}

const totalPages = computed(() =>
  Math.ceil(totalRecords.value / pageSize.value),
);

const hasActiveFilters = computed(() => {
  return (
    filters.api_key_id !== 0 ||
    filters.provider_id !== 0 ||
    filters.status !== "ALL" ||
    filters.search !== ""
  );
});

const apiKeyOptions = computed(() => {
  const allKey = { value: 0, label: $t("recordPage.filter.allApiKeys") };
  const keys = (apiKeyStore.apiKeys || []).map((k) => ({
    value: k.id,
    label: k.name,
  }));
  return [allKey, ...keys];
});

const providerOptions = computed(() => {
  const allProvider = { value: 0, label: $t("recordPage.filter.allProviders") };
  const providers = (providerStore.providers || []).map((p) => ({
    value: p.id,
    label: p.name,
  }));
  return [allProvider, ...providers];
});

const statusOptions = computed(() => {
  const allStatus = {
    value: DEFAULT_FILTERS.status,
    label: $t("recordPage.filter.allStatuses"),
  };
  const statuses = ["SUCCESS", "PENDING", "ERROR"].map((s) => ({
    value: s,
    label: $t(`recordPage.filter.status.${s}`),
  }));
  return [allStatus, ...statuses];
});

const mobileFilterSummary = computed(() => {
  const summary = [
    filters.api_key_id !== 0
      ? apiKeyOptions.value.find((item) => item.value === filters.api_key_id)?.label
      : null,
    filters.provider_id !== 0
      ? providerOptions.value.find((item) => item.value === filters.provider_id)?.label
      : null,
    filters.status !== DEFAULT_FILTERS.status
      ? statusOptions.value.find((item) => item.value === filters.status)?.label
      : null,
    filters.search ? `Search: ${filters.search}` : null,
  ].filter(Boolean);

  if (summary.length === 0) {
    return "All records, no extra filters applied.";
  }

  return summary.join(" · ");
});

const getSingleQueryValue = (value: LocationQuery[string]) => {
  if (Array.isArray(value)) return value[0];
  return value;
};

const parsePositiveIntQuery = (value: LocationQuery[string], fallback: number) => {
  const raw = getSingleQueryValue(value);
  if (raw == null || raw === "") return fallback;
  const parsed = Number(raw);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
};

const parseStatusQuery = (value: LocationQuery[string]) => {
  const raw = getSingleQueryValue(value);
  return raw && VALID_STATUSES.has(raw) ? raw : DEFAULT_FILTERS.status;
};

const parseSearchQuery = (value: LocationQuery[string]) => {
  const raw = getSingleQueryValue(value);
  return typeof raw === "string" ? raw : DEFAULT_FILTERS.search;
};

const hasProviderId = (id: number) => {
  return providerStore.providers.some((item) => item.id === id);
};

const hasApiKeyId = (id: number) => {
  return apiKeyStore.apiKeys.some((item) => item.id === id);
};

const applyQueryToState = (query: LocationQuery) => {
  currentPage.value = parsePositiveIntQuery(query.page, DEFAULT_PAGE);
  pageSize.value = parsePositiveIntQuery(query.page_size, getStoredPageSize());
  localStorage.setItem("pageSize", String(pageSize.value));

  const providerId = parsePositiveIntQuery(
    query.provider_id,
    DEFAULT_FILTERS.provider_id,
  );
  const apiKeyId = parsePositiveIntQuery(
    query.api_key_id,
    DEFAULT_FILTERS.api_key_id,
  );

  filters.provider_id =
    providerId > 0 && hasProviderId(providerId)
      ? providerId
      : DEFAULT_FILTERS.provider_id;
  filters.api_key_id =
    apiKeyId > 0 && hasApiKeyId(apiKeyId)
      ? apiKeyId
      : DEFAULT_FILTERS.api_key_id;
  filters.status = parseStatusQuery(query.status);
  filters.search = parseSearchQuery(query.search);
  searchInput.value = filters.search;
};

const buildQueryFromState = () => {
  const query: Record<string, string> = {};

  if (currentPage.value !== DEFAULT_PAGE) {
    query.page = String(currentPage.value);
  }
  if (pageSize.value !== FALLBACK_PAGE_SIZE) {
    query.page_size = String(pageSize.value);
  }
  if (filters.provider_id !== DEFAULT_FILTERS.provider_id) {
    query.provider_id = String(filters.provider_id);
  }
  if (filters.api_key_id !== DEFAULT_FILTERS.api_key_id) {
    query.api_key_id = String(filters.api_key_id);
  }
  if (filters.status !== DEFAULT_FILTERS.status) {
    query.status = filters.status;
  }
  if (filters.search !== DEFAULT_FILTERS.search) {
    query.search = filters.search;
  }

  return query;
};

const isSameQuery = (
  currentQuery: LocationQuery,
  nextQuery: Record<string, string>,
) => {
  const currentEntries = Object.entries(currentQuery)
    .map(([key, value]) => [key, getSingleQueryValue(value) ?? ""])
    .filter(([, value]) => value !== "")
    .sort(([left], [right]) => left.localeCompare(right));
  const nextEntries = Object.entries(nextQuery).sort(([left], [right]) =>
    left.localeCompare(right),
  );

  if (currentEntries.length !== nextEntries.length) return false;

  return currentEntries.every(([key, value], index) => {
    const [nextKey, nextValue] = nextEntries[index];
    return key === nextKey && value === nextValue;
  });
};

const syncRouteWithState = async () => {
  const nextQuery = buildQueryFromState();
  if (isSameQuery(route.query, nextQuery)) {
    return false;
  }
  await router.replace({ query: nextQuery });
  return true;
};

const fetchRecords = async () => {
  isLoading.value = true;
  errorMsg.value = null;

  try {
    const params = {
      page: currentPage.value,
      page_size: pageSize.value,
      api_key_id: filters.api_key_id || undefined,
      provider_id: filters.provider_id || undefined,
      status:
        filters.status === DEFAULT_FILTERS.status ? undefined : filters.status,
      search: filters.search || undefined,
    };
    const result = await Api.getRecordList(params);

    records.value = (result.list || []).map((r: RecordListItem) => {
      const providerName =
        r.provider_id != null
          ? providerStore.providers.find((p) => p.id === r.provider_id)?.name || "/"
          : "/";
      const apiKeyName =
        r.api_key_id != null
          ? apiKeyStore.apiKeys.find((k) => k.id === r.api_key_id)?.name || "/"
          : "/";
      const isStreamDisplay = r.is_stream ? $t("common.yes") : $t("common.no");

      const firstRespTimeDisplay =
        r.llm_response_first_chunk_at != null && r.llm_request_sent_at != null
          ? ((r.llm_response_first_chunk_at - r.llm_request_sent_at) / 1000).toFixed(3)
          : "/";
      const totalRespTimeDisplay =
        r.llm_response_completed_at != null && r.llm_request_sent_at != null
          ? ((r.llm_response_completed_at - r.llm_request_sent_at) / 1000).toFixed(3)
          : "/";

      let tpsDisplay = "/";
      if (r.total_output_tokens != null && r.llm_response_completed_at != null) {
        let durationMs;
        if (r.is_stream) {
          if (r.llm_response_first_chunk_at != null) {
            durationMs =
              r.llm_response_completed_at - r.llm_response_first_chunk_at;
          }
        } else if (r.llm_request_sent_at != null) {
          durationMs = r.llm_response_completed_at - r.llm_request_sent_at;
        }

        if (durationMs != null && durationMs > 0) {
          tpsDisplay = (r.total_output_tokens / (durationMs / 1000)).toFixed(2);
        }
      }

      const costDisplay = formatPriceFromNanos(
        r.estimated_cost_nanos,
        r.estimated_cost_currency,
        "/",
      );

      return {
        ...r,
        providerName,
        apiKeyName,
        displayRequestedModelName: r.requested_model_name || r.model_name || "/",
        resolvedScopeDisplay: formatResolvedScope(r.resolved_name_scope),
        isStreamDisplay,
        firstRespTimeDisplay,
        totalRespTimeDisplay,
        tpsDisplay,
        costDisplay,
        request_at_formatted: formatDate(r.request_received_at),
      };
    });
    totalRecords.value = result.total || 0;
  } catch (err: unknown) {
    console.error("Failed to fetch records:", err);
    errorMsg.value = (err as Error).message || String(err);
  } finally {
    isLoading.value = false;
  }
};

const closeMobileFilterPanel = () => {
  isFilterPanelOpen.value = false;
};

const handleApplyFilter = () => {
  filters.search = searchInput.value.trim();
  currentPage.value = DEFAULT_PAGE;
  closeMobileFilterPanel();
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleClearSearch = () => {
  if (!searchInput.value && !filters.search) return;
  searchInput.value = DEFAULT_FILTERS.search;
  filters.search = DEFAULT_FILTERS.search;
  currentPage.value = DEFAULT_PAGE;
  closeMobileFilterPanel();
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleApiKeyFilterChange = (val: unknown) => {
  const nextId = Number(val);
  filters.api_key_id =
    Number.isInteger(nextId) && nextId >= 0 ? nextId : DEFAULT_FILTERS.api_key_id;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleProviderFilterChange = (val: unknown) => {
  const nextId = Number(val);
  filters.provider_id =
    Number.isInteger(nextId) && nextId >= 0 ? nextId : DEFAULT_FILTERS.provider_id;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleStatusFilterChange = (val: unknown) => {
  const nextStatus =
    typeof val === "string" && VALID_STATUSES.has(val)
      ? val
      : DEFAULT_FILTERS.status;
  filters.status = nextStatus;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleResetFilter = () => {
  searchInput.value = DEFAULT_FILTERS.search;
  filters.api_key_id = DEFAULT_FILTERS.api_key_id;
  filters.provider_id = DEFAULT_FILTERS.provider_id;
  filters.status = DEFAULT_FILTERS.status;
  filters.search = DEFAULT_FILTERS.search;
  currentPage.value = DEFAULT_PAGE;
  closeMobileFilterPanel();
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handlePageChange = (page: number) => {
  currentPage.value = page;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handlePageSizeChange = (val: unknown) => {
  const size = Number(val);
  if (!Number.isInteger(size) || size <= 0) return;
  pageSize.value = size;
  localStorage.setItem("pageSize", String(size));
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleViewDetails = async (id: number) => {
  isDetailModalOpen.value = true;
  isDetailLoading.value = true;
  detailedRecord.value = null;
  showPayloads.value = false;

  try {
    detailedRecord.value = await Api.getRecordDetail(id);
    await nextTick();
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (isDetailModalOpen.value && detailedRecord.value?.id === id) {
          showPayloads.value = true;
        }
      });
    });
  } catch (err: unknown) {
    console.error("Failed to fetch record detail:", err);
    toastController.error(
      $t("recordPage.detailModal.fetchFailed", "Failed to fetch record detail"),
      (err as Error).message || String(err),
    );
  } finally {
    isDetailLoading.value = false;
  }
};

const toggleFilterPanel = () => {
  isFilterPanelOpen.value = !isFilterPanelOpen.value;
};

watch(isDetailModalOpen, (isOpen) => {
  if (!isOpen) {
    showPayloads.value = false;
  }
});

const formatDate = (timestamp: number | null | undefined) => {
  return formatTimestamp(timestamp) || "/";
};

const formatResolvedScope = (scope: string | null | undefined) => {
  switch (scope) {
    case "direct":
      return "Direct";
    case "global_route":
      return "Global Route";
    case "api_key_override":
      return "API Key Override";
    default:
      return "/";
  }
};

const formatCompactMetric = (value: number | string | null | undefined) => {
  if (value == null || value === "" || value === "/") {
    return "-";
  }
  return String(value);
};

const formatCompactMetrics = (
  values: Array<number | string | null | undefined>,
) => {
  return values.map(formatCompactMetric).join(" / ");
};

const getStatusBadgeVariant = (status: string | null) => {
  switch (status) {
    case "SUCCESS":
      return "default";
    case "ERROR":
      return "destructive";
    case "PENDING":
      return "outline";
    default:
      return "secondary";
  }
};

const getStatusMeta = (status: string | null) => {
  switch (status) {
    case "SUCCESS":
      return {
        icon: CircleCheckBig,
        className: "text-emerald-600",
        label: $t("recordPage.filter.status.SUCCESS"),
      };
    case "ERROR":
      return {
        icon: CircleAlert,
        className: "text-red-600",
        label: $t("recordPage.filter.status.ERROR"),
      };
    case "PENDING":
      return {
        icon: Clock3,
        className: "text-amber-600",
        label: $t("recordPage.filter.status.PENDING"),
      };
    default:
      return {
        icon: CircleHelp,
        className: "text-gray-400",
        label: status || $t("common.notAvailable"),
      };
  }
};

const getProviderName = (id: number | null) => {
  if (id == null) return "/";
  return (
    providerStore.providers.find((p) => p.id === id)?.name || "/"
  );
};

const getApiKeyName = (id: number | null) => {
  if (id == null) return "/";
  return apiKeyStore.apiKeys.find((k) => k.id === id)?.name || "/";
};

watch(
  () => route.query,
  async (query) => {
    applyQueryToState(query);
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  },
);

onMounted(async () => {
  try {
    await Promise.all([
      providerStore.fetchProviders(),
      apiKeyStore.fetchApiKeys(),
    ]);
    applyQueryToState(route.query);
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  } catch (error: unknown) {
    errorMsg.value = normalizeError(error, $t("common.unknownError")).message;
  }
});
</script>
