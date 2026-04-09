<template>
  <div class="p-6 space-y-6">
    <!-- 页面头部 -->
    <div class="flex justify-between items-start">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">
          {{ $t("providerPage.title") }}
        </h1>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("providerPage.description") }}
        </p>
      </div>
      <Button @click="router.push('/provider/new')" variant="outline">
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("providerPage.addProvider") }}
      </Button>
    </div>

    <!-- 加载/错误/空状态 -->
    <div
      v-if="isLoading"
      class="flex items-center justify-center py-16 border border-gray-200 rounded-lg bg-white"
    >
      <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
      <span class="text-sm text-gray-500">{{ $t("providerPage.loading") }}</span>
    </div>

    <div
      v-else-if="error"
      class="flex flex-col items-center justify-center py-20 border border-gray-200 rounded-lg bg-white"
    >
      <AlertCircle class="h-10 w-10 stroke-1 text-red-400 mb-4" />
      <span class="text-sm font-medium text-red-500">{{
        $t("providerPage.error", { error: error })
      }}</span>
    </div>

    <div
      v-else-if="store.providers.length === 0"
      class="flex flex-col items-center justify-center py-20 border border-gray-200 rounded-lg bg-white"
    >
      <Inbox class="h-10 w-10 stroke-1 text-gray-400 mb-4" />
      <span class="text-sm font-medium text-gray-500">{{
        $t("providerPage.noData")
      }}</span>
    </div>

    <!-- Card Grid (Optimized Compact) -->
    <div
      v-else
      class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4"
    >
      <div
        v-for="item in store.providers"
        :key="item.provider.id"
        class="bg-white rounded-lg border border-gray-200 flex flex-col h-full hover:border-gray-300 transition-colors"
      >
        <!-- Card Header -->
        <div
          class="px-4 py-3.5 border-b border-gray-100 flex justify-between items-start"
        >
          <div>
            <div class="flex items-center space-x-2">
              <h3 class="text-sm font-semibold text-gray-900">
                {{ item.provider.name }}
              </h3>
              <Badge
                variant="secondary"
                class="font-mono text-[10px] px-1.5 py-0"
                >{{ item.provider.provider_type }}</Badge
              >
            </div>
            <p class="text-xs text-gray-400 font-mono mt-0.5">
              {{ item.provider.provider_key }}
            </p>
          </div>
        </div>

        <!-- Card Content -->
        <div class="p-4 flex-grow space-y-4">
          <!-- Meta Data -->
          <div class="flex flex-col space-y-2 text-xs">
            <div class="flex items-center justify-between text-gray-500">
              <span class="flex items-center">
                <Key class="w-3.5 h-3.5 mr-1.5 text-gray-400" />
                {{ $t("providerPage.table.apiKeys") }}
              </span>
              <span class="text-gray-900 font-medium">{{
                item.provider_keys.length
              }}</span>
            </div>
            <div class="flex items-center justify-between text-gray-500">
              <span class="flex items-center">
                <Network class="w-3.5 h-3.5 mr-1.5 text-gray-400" />
                {{ $t("providerPage.table.useProxy") }}
              </span>
              <div class="flex items-center text-gray-700 font-medium">
                <CheckCircle2
                  v-if="item.provider.use_proxy"
                  class="w-3.5 h-3.5 mr-1 text-gray-600"
                />
                <XCircle v-else class="w-3.5 h-3.5 mr-1 text-gray-300" />
                {{
                  item.provider.use_proxy ? $t("common.yes") : $t("common.no")
                }}
              </div>
            </div>
          </div>

          <!-- Models List (Truncated further) -->
          <div>
            <span class="text-gray-500 mb-2.5 flex items-center text-xs">
              <Box class="w-3.5 h-3.5 mr-1.5 text-gray-400" />
              {{ $t("providerPage.table.models") }} ({{ item.models.length }})
            </span>
            <div class="flex flex-wrap gap-1.5 mt-1.5">
              <Badge
                v-for="modelDetail in item.models.slice(0, 6)"
                :key="modelDetail.model.id"
                variant="secondary"
                class="font-mono font-normal text-[11px] px-1.5 py-0 hover:bg-gray-200 hover:text-gray-900 cursor-pointer text-gray-500 transition-colors"
                :title="
                  $t('providerPage.editModel', {
                    model_name: modelDetail.model.model_name,
                  })
                "
                @click="router.push(`/model/edit/${modelDetail.model.id}`)"
              >
                {{ modelDetail.model.model_name }}
              </Badge>
              <Badge
                v-if="item.models.length > 6"
                variant="outline"
                class="text-[11px] px-1.5 py-0 text-gray-400 bg-gray-50 border-gray-100"
              >
                +{{ item.models.length - 6 }}
              </Badge>
            </div>
          </div>
        </div>

        <!-- Card Footer -->
        <div
          class="px-3 py-2 border-t border-gray-100 flex justify-end space-x-1"
        >
          <Button
            variant="ghost"
            size="sm"
            @click="router.push(`/provider/edit/${item.provider.id}`)"
          >
            <Pencil class="w-3.5 h-3.5 mr-1.5 text-gray-500" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="text-gray-400 hover:text-red-600 hover:bg-red-50"
            @click="handleDeleteProvider(item.provider)"
          >
            <Trash2 class="w-3.5 h-3.5 mr-1.5" />
            {{ $t("common.delete") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { useProviderStore } from "@/store/providerStore";
import type { ProviderBase } from "@/store/types";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { confirm } from "@/lib/confirmController";
import { Api } from "@/services/request";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Plus,
  Loader2,
  AlertCircle,
  Inbox,
  Key,
  Network,
  Box,
  CheckCircle2,
  XCircle,
  Pencil,
  Trash2,
} from "lucide-vue-next";

const { t: $t } = useI18n();
const router = useRouter();
const store = useProviderStore();

const isLoading = ref(true);
const error = ref<string | null>(null);

const loadData = async () => {
  isLoading.value = true;
  error.value = null;
  try {
    await store.fetchProviders();
  } catch (err: unknown) {
    error.value = normalizeError(
      err,
      $t("common.unknownError", "Unknown Error"),
    ).message;
  } finally {
    isLoading.value = false;
  }
};

onMounted(() => {
  loadData();
});

const handleDeleteProvider = async (provider: ProviderBase) => {
  await confirm($t("providerPage.confirmDelete", { name: provider.name }))
  try {
    await Api.deleteProvider(provider.id);
    toastController.success($t("deleteSuccess", "Deleted successfully"));
    await loadData();
  } catch (err: any) {
    console.error("Failed to delete provider:", err);
    const errorMessage = err.message || $t("common.unknownError", "Unknown Error");
    toastController.error($t("providerPage.deleteFailed", { error: errorMessage }));
  } 
};
</script>
