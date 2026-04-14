<script setup lang="ts">
import { Copy, Plus, Sparkles, Trash2 } from "lucide-vue-next";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatTimestamp } from "@/lib/utils";
import type { CostCatalogListItem } from "@/store/types";

defineProps<{
  catalogs: CostCatalogListItem[];
  isLoading: boolean;
  selectedCatalogId: number | null;
  duplicatingCatalogId: number | null;
}>();

const emit = defineEmits<{
  (e: "open-template"): void;
  (e: "refresh"): void;
  (e: "create-catalog"): void;
  (e: "open-catalog", catalogId: number): void;
  (e: "duplicate-catalog", catalog: CostCatalogListItem): void;
  (e: "delete-catalog", catalogId: number, name: string): void;
}>();
</script>

<template>
  <div class="space-y-6">
    <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
      <div>
        <h2 class="text-lg font-medium text-gray-900">
          {{ $t("costPage.catalogs.title") }}
        </h2>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("costPage.catalogs.description") }}
        </p>
      </div>
      <div class="flex flex-col gap-2 sm:flex-row">
        <Button variant="outline" @click="emit('open-template')">
          <Sparkles class="mr-1.5 h-4 w-4" />
          {{ $t("costPage.templates.title") }}
        </Button>
        <Button variant="outline" @click="emit('refresh')">
          {{ $t("common.refresh") }}
        </Button>
        <Button @click="emit('create-catalog')">
          <Plus class="mr-1.5 h-4 w-4" />
          {{ $t("costPage.catalogs.add") }}
        </Button>
      </div>
    </div>

    <div
      v-if="isLoading"
      class="rounded-xl border border-dashed border-gray-200 bg-gray-50/60 px-6 py-12 text-center text-sm text-gray-500"
    >
      {{ $t("costPage.catalogs.loading") }}
    </div>
    <template v-else>
      <div class="grid grid-cols-1 gap-3 xl:hidden">
        <MobileCrudCard
          v-for="item in catalogs"
          :key="item.catalog.id"
          :title="item.catalog.name"
          :description="item.catalog.description || $t('costPage.catalogs.emptyDescription')"
          :selected="selectedCatalogId === item.catalog.id"
        >
          <template #header>
            <Badge variant="outline" class="text-[11px]">
              {{ item.versions.length }} {{ $t("costPage.catalogs.versionCount") }}
            </Badge>
          </template>

          <div class="grid grid-cols-2 gap-2 text-xs text-gray-500">
            <div class="rounded-lg border border-gray-100 px-3 py-2.5">
              <div>{{ $t("costPage.catalogs.latestVersion") }}</div>
              <div class="mt-1 font-mono text-sm text-gray-900">
                {{ item.versions[0]?.version || "-" }}
              </div>
            </div>
            <div class="rounded-lg border border-gray-100 px-3 py-2.5">
              <div>{{ $t("costPage.catalogs.latestPublishedAt") }}</div>
              <div class="mt-1 text-gray-900">
                {{ formatTimestamp(item.versions[0]?.created_at) || "-" }}
              </div>
            </div>
          </div>

          <template #actions>
            <Button
              :variant="selectedCatalogId === item.catalog.id ? 'default' : 'outline'"
              size="sm"
              class="w-full"
              @click="emit('open-catalog', item.catalog.id)"
            >
              {{
                selectedCatalogId === item.catalog.id
                  ? $t("costPage.catalogs.openCurrent")
                  : $t("costPage.catalogs.openEditor")
              }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="w-full"
              :disabled="duplicatingCatalogId === item.catalog.id"
              @click="emit('duplicate-catalog', item)"
            >
              <Copy class="mr-1 h-3.5 w-3.5" />
              {{
                duplicatingCatalogId === item.catalog.id
                  ? $t("common.loading")
                  : $t("costPage.catalogs.duplicate")
              }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="w-full text-gray-500 hover:text-red-600"
              @click="emit('delete-catalog', item.catalog.id, item.catalog.name)"
            >
              <Trash2 class="mr-1 h-3.5 w-3.5" />
              {{ $t("common.delete") }}
            </Button>
          </template>
        </MobileCrudCard>
      </div>

      <div class="hidden overflow-hidden rounded-xl border border-gray-200 xl:block">
        <Table>
          <TableHeader>
            <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
              <TableHead>{{ $t("costPage.catalogs.table.name") }}</TableHead>
              <TableHead>{{ $t("costPage.catalogs.table.description") }}</TableHead>
              <TableHead>{{ $t("costPage.catalogs.table.versions") }}</TableHead>
              <TableHead>{{ $t("costPage.catalogs.table.latestVersion") }}</TableHead>
              <TableHead>{{ $t("costPage.catalogs.table.latestPublishedAt") }}</TableHead>
              <TableHead class="text-right">{{ $t("common.actions") }}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="item in catalogs"
              :key="item.catalog.id"
              class="cursor-pointer transition-colors"
              :class="{
                'bg-gray-100 font-medium': selectedCatalogId === item.catalog.id,
              }"
              @click="emit('open-catalog', item.catalog.id)"
            >
              <TableCell class="align-top">
                <div class="font-medium text-gray-900">
                  {{ item.catalog.name }}
                </div>
              </TableCell>
              <TableCell class="align-top">
                <div class="max-w-[28rem] whitespace-normal break-words text-sm leading-6 text-gray-600">
                  {{ item.catalog.description || $t("costPage.catalogs.emptyDescription") }}
                </div>
              </TableCell>
              <TableCell class="w-20 align-top">{{ item.versions.length }}</TableCell>
              <TableCell class="w-32 align-top font-mono text-sm">
                {{ item.versions[0]?.version || "-" }}
              </TableCell>
              <TableCell class="w-40 align-top">
                {{ formatTimestamp(item.versions[0]?.created_at) || "-" }}
              </TableCell>
              <TableCell class="w-56 text-right align-top">
                <div class="flex justify-end gap-1 whitespace-nowrap">
                <Button
                  variant="ghost"
                  size="sm"
                  @click.stop="emit('open-catalog', item.catalog.id)"
                >
                  {{ $t("costPage.catalogs.openEditor") }}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  :disabled="duplicatingCatalogId === item.catalog.id"
                  @click.stop="emit('duplicate-catalog', item)"
                >
                  <Copy class="mr-1 h-3.5 w-3.5" />
                  {{ $t("costPage.catalogs.duplicate") }}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="text-gray-500 hover:text-red-600"
                  @click.stop="emit('delete-catalog', item.catalog.id, item.catalog.name)"
                >
                  <Trash2 class="mr-1 h-3.5 w-3.5" />
                  {{ $t("common.delete") }}
                </Button>
                </div>
              </TableCell>
            </TableRow>
            <TableRow v-if="catalogs.length === 0">
              <TableCell colspan="6" class="py-12 text-center text-sm text-gray-500">
                {{ $t("common.noData") }}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </template>
  </div>
</template>
