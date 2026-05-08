<script setup lang="ts">
import {
  BrainCircuit,
  Pencil,
  Trash2,
} from "lucide-vue-next";

import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { ModelRouteListItem } from "@/services/types";

defineProps<{
  routes: ModelRouteListItem[];
  actionRouteId?: number | null;
}>();

const emit = defineEmits<{
  edit: [id: number];
  delete: [item: ModelRouteListItem];
  reasoning: [id: number];
  toggleEnabled: [item: ModelRouteListItem, isEnabled: boolean];
  toggleExpose: [item: ModelRouteListItem, exposeInModels: boolean];
}>();
</script>

<template>
  <div class="grid grid-cols-1 gap-3 md:hidden">
    <MobileCrudCard
      v-for="item in routes"
      :key="item.route.id"
      :title="item.route.route_name"
      :description="item.route.description || $t('modelRoutePage.queue.candidateCount', { count: item.candidate_count })"
    >
      <template #header>
        <div class="flex flex-wrap items-center gap-2">
          <Badge :variant="item.route.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
            {{ item.route.is_enabled ? $t("common.yes") : $t("common.no") }}
          </Badge>
          <Badge :variant="item.route.expose_in_models ? 'secondary' : 'outline'" class="font-mono text-[11px]">
            /models: {{ item.route.expose_in_models ? $t("common.yes") : $t("common.no") }}
          </Badge>
        </div>
      </template>

      <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ $t("modelRoutePage.table.candidateCount") }}</span>
          <span class="font-medium text-gray-700">{{ item.candidate_count }}</span>
        </div>
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ $t("modelRoutePage.table.enabled") }}</span>
          <Checkbox
            :model-value="item.route.is_enabled"
            :disabled="actionRouteId === item.route.id"
            @update:model-value="(value) => emit('toggleEnabled', item, value === true)"
          />
        </div>
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ $t("modelRoutePage.table.exposeInModels") }}</span>
          <Checkbox
            :model-value="item.route.expose_in_models"
            :disabled="actionRouteId === item.route.id"
            @update:model-value="(value) => emit('toggleExpose', item, value === true)"
          />
        </div>
      </div>

      <template #actions>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-center"
          @click="emit('reasoning', item.route.id)"
        >
          <BrainCircuit class="mr-1 h-3.5 w-3.5" />
          {{ $t("modelRoutePage.table.reasoning") }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-center"
          @click="emit('edit', item.route.id)"
        >
          <Pencil class="mr-1 h-3.5 w-3.5" />
          {{ $t("common.edit") }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-center text-gray-400 hover:text-red-600"
          @click="emit('delete', item)"
        >
          <Trash2 class="mr-1 h-3.5 w-3.5" />
          {{ $t("common.delete") }}
        </Button>
      </template>
    </MobileCrudCard>
  </div>

  <div class="hidden overflow-hidden rounded-lg border border-gray-200 bg-white md:block">
    <Table>
      <TableHeader>
        <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ $t("modelRoutePage.table.routeName") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ $t("modelRoutePage.table.candidateCount") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ $t("modelRoutePage.table.exposeInModels") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ $t("modelRoutePage.table.enabled") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ $t("modelRoutePage.table.reasoning") }}
          </TableHead>
          <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ $t("common.actions") }}
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow v-for="item in routes" :key="item.route.id">
          <TableCell>
            <div class="min-w-0">
              <div class="font-medium text-gray-900">{{ item.route.route_name }}</div>
              <div v-if="item.route.description" class="mt-0.5 text-xs text-gray-500">
                {{ item.route.description }}
              </div>
            </div>
          </TableCell>
          <TableCell>
            <Badge variant="outline" class="font-mono text-xs">
              {{ item.candidate_count }}
            </Badge>
          </TableCell>
          <TableCell>
            <Checkbox
              :model-value="item.route.expose_in_models"
              :disabled="actionRouteId === item.route.id"
              @update:model-value="(value) => emit('toggleExpose', item, value === true)"
            />
          </TableCell>
          <TableCell>
            <Checkbox
              :model-value="item.route.is_enabled"
              :disabled="actionRouteId === item.route.id"
              @update:model-value="(value) => emit('toggleEnabled', item, value === true)"
            />
          </TableCell>
          <TableCell>
            <Button variant="ghost" size="sm" @click="emit('reasoning', item.route.id)">
              <BrainCircuit class="mr-1 h-3.5 w-3.5" />
              {{ $t("modelRoutePage.table.preview") }}
            </Button>
          </TableCell>
          <TableCell class="text-right">
            <Button variant="ghost" size="sm" @click="emit('edit', item.route.id)">
              <Pencil class="mr-1 h-3.5 w-3.5" />
              {{ $t("common.edit") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="text-gray-400 hover:text-red-600"
              @click="emit('delete', item)"
            >
              <Trash2 class="mr-1 h-3.5 w-3.5" />
              {{ $t("common.delete") }}
            </Button>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>
