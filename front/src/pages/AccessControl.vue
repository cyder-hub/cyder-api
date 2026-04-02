<template>
  <CrudPageLayout
    :title="$t('accessControlPage.title')"
    :description="$t('accessControlPage.description')"
    :loading="storeLoading"
    :error="pageError"
    :empty="!store.policies.length"
  >
    <template #actions>
      <Button
        @click="handleOpenAddModal"
        variant="outline"
        :disabled="providersLoading || storeLoading"
      >
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("accessControlPage.addPolicy") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center py-16">
        <Loader2 class="h-5 w-5 animate-spin mr-2" />
        <span class="text-sm text-gray-500">{{
          $t("accessControlPage.loading")
        }}</span>
      </div>
    </template>

    <template #error="{ error }">
      <div class="flex flex-col items-center justify-center py-20">
        <div
          class="text-red-600 bg-red-50 border border-red-200 rounded-lg p-4 max-w-lg text-sm"
        >
          {{ error }}
        </div>
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20">
        <ShieldAlert class="h-10 w-10 stroke-1 text-gray-400 mb-2" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("accessControlPage.noData")
        }}</span>
      </div>
    </template>

    <div class="border border-gray-200 rounded-lg overflow-hidden">
      <Table>
        <TableHeader>
          <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("accessControlPage.table.name") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("accessControlPage.table.defaultAction") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("accessControlPage.table.description") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("accessControlPage.table.rules") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
              >{{ $t("accessControlPage.table.actions") }}</TableHead
            >
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="policy in store.policies" :key="policy.id">
            <TableCell class="text-sm font-medium text-gray-900">{{
              policy.name
            }}</TableCell>
            <TableCell>
              <Badge variant="secondary" class="font-mono text-xs">{{
                $t(`accessControlPage.modal.option${policy.default_action}`)
              }}</Badge>
            </TableCell>
            <TableCell class="text-sm text-gray-600">{{
              policy.description || "/"
            }}</TableCell>
            <TableCell class="text-sm text-gray-600">{{
              $t("accessControlPage.table.rulesCount", {
                count: policy.rules?.length || 0,
              })
            }}</TableCell>
            <TableCell class="text-right">
              <Button
                variant="ghost"
                size="sm"
                @click="handleOpenEditModal(policy.id)"
                :disabled="providersLoading || storeLoading"
              >
                <Edit2 class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.edit") }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600"
                @click="handleDeletePolicy(policy.id, policy.name)"
              >
                <Trash2 class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.delete") }}
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <template #modals>
      <!-- Edit/Add Modal -->
      <Dialog :open="showEditModal" @update:open="setShowEditModal">
        <DialogContent
          class="max-w-4xl max-h-[90vh] flex flex-col p-6 overflow-hidden"
        >
          <DialogHeader>
            <DialogTitle class="text-lg font-semibold text-gray-900">{{
              editingPolicy.id
                ? $t("accessControlPage.modal.titleEdit")
                : $t("accessControlPage.modal.titleAdd")
            }}</DialogTitle>
          </DialogHeader>

          <div class="space-y-4 overflow-y-auto flex-1 pr-2">
          <!-- Policy Fields -->
          <div class="space-y-4">
            <div>
              <Label class="text-gray-700"
                >{{ $t("accessControlPage.modal.labelName")
                }}<span class="text-red-500 ml-0.5">*</span></Label
              >
              <Input class="mt-1.5" v-model="editingPolicy.name" />
            </div>

            <div class="grid grid-cols-2 gap-4">
              <div>
                <Label class="text-gray-700">{{
                  $t("accessControlPage.modal.labelDefaultAction")
                }}</Label>
                <Select v-model="editingPolicy.default_action">
                  <SelectTrigger class="w-full mt-1.5">
                    <SelectValue
                      :placeholder="
                        $t('accessControlPage.modal.placeholderDefaultAction')
                      "
                    />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem
                      v-for="opt in defaultActionOptions"
                      :key="opt.value"
                      :value="opt.value"
                    >
                      {{ opt.label }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div>
                <Label class="text-gray-700">{{
                  $t("accessControlPage.modal.labelDescription")
                }}</Label>
                <Input class="mt-1.5" v-model="editingPolicy.description" />
              </div>
            </div>
          </div>

          <hr class="my-6 border-gray-100" />

          <!-- Rules Section -->
          <div class="space-y-4">
            <div class="flex justify-between items-center">
              <h4 class="text-sm font-medium text-gray-900">
                {{ $t("accessControlPage.rules.title") }}
              </h4>
              <Button @click="addRule" variant="outline" size="sm">
                <Plus class="h-3.5 w-3.5 mr-1" />
                {{ $t("accessControlPage.rules.addRule") }}
              </Button>
            </div>

            <div
              class="max-h-80 overflow-y-auto border border-gray-200 rounded-lg p-2 space-y-2 bg-gray-50"
            >
              <div
                v-if="!editingPolicy.rules.length"
                class="flex flex-col items-center justify-center py-10"
              >
                <span class="text-sm font-medium text-gray-500">{{
                  $t("accessControlPage.rules.noRules")
                }}</span>
              </div>

              <div
                v-for="(rule, index) in editingPolicy.rules"
                :key="index"
                class="p-5 bg-white border border-gray-200 rounded-lg"
              >
                <div
                  class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5 items-end"
                >
                  <!-- Rule Type -->
                  <div>
                    <Label class="text-gray-700 block mb-1.5">{{
                      $t("accessControlPage.rules.labelRuleType")
                    }}</Label>
                    <Select v-model="rule.rule_type">
                      <SelectTrigger
                        class="w-full focus:ring-1 focus:ring-gray-300"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem
                          v-for="opt in ruleTypeOptions"
                          :key="opt.value"
                          :value="opt.value"
                        >
                          {{ opt.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <!-- Scope -->
                  <div>
                    <Label class="text-gray-700 block mb-1.5">{{
                      $t("accessControlPage.rules.labelScope")
                    }}</Label>
                    <Select
                      v-model="rule.scope"
                      @update:model-value="onScopeChange(rule)"
                    >
                      <SelectTrigger
                        class="w-full focus:ring-1 focus:ring-gray-300"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem
                          v-for="opt in scopeOptions"
                          :key="opt.value"
                          :value="opt.value"
                        >
                          {{ opt.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <!-- Provider -->
                  <div class="md:col-span-2 lg:col-span-2">
                    <Label class="text-gray-700 block mb-1.5 truncate">{{
                      $t("accessControlPage.rules.labelProvider")
                    }}</Label>
                    <Select
                      v-model="rule.provider_id"
                      @update:model-value="onProviderChange(rule)"
                    >
                      <SelectTrigger
                        class="w-full focus:ring-1 focus:ring-gray-300"
                      >
                        <SelectValue
                          :placeholder="
                            $t('accessControlPage.rules.placeholderProvider')
                          "
                          class="truncate"
                        />
                      </SelectTrigger>
                      <SelectContent>
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

                  <!-- Model -->
                  <div
                    v-if="rule.scope === 'MODEL'"
                    class="md:col-span-2 lg:col-span-2"
                  >
                    <Label class="text-gray-700 block mb-1.5 truncate">{{
                      $t("accessControlPage.rules.labelModel")
                    }}</Label>
                    <Select
                      v-model="rule.model_id"
                      :disabled="!rule.provider_id"
                    >
                      <SelectTrigger
                        class="w-full focus:ring-1 focus:ring-gray-300"
                      >
                        <SelectValue
                          :placeholder="
                            $t('accessControlPage.rules.placeholderModel')
                          "
                          class="truncate"
                        />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem
                          v-for="opt in getModelOptions(rule.provider_id)"
                          :key="opt.value"
                          :value="String(opt.value)"
                        >
                          {{ opt.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <!-- Priority -->
                  <div>
                    <Label class="text-gray-700 block mb-1.5 truncate">{{
                      $t("accessControlPage.rules.labelPriority")
                    }}</Label>
                    <Input
                      class="w-full focus:ring-1 focus:ring-gray-300"
                      type="number"
                      v-model.number="rule.priority"
                    />
                  </div>

                  <!-- Actions -->
                  <div>
                    <Button
                      type="button"
                      @click="removeRule(index)"
                      variant="outline"
                      class="w-full text-red-600 border-red-200 hover:bg-red-50 hover:text-red-700 transition-colors"
                    >
                      <Trash2 class="h-4 w-4 mr-2" />
                      {{ $t("accessControlPage.rules.deleteRule") }}
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

          <DialogFooter class="border-t border-gray-100 pt-4 mt-2">
            <Button
              @click="handleCloseModal"
              variant="ghost"
              class="text-gray-600"
              >{{ $t("common.cancel") }}</Button
            >
            <Button @click="handleSavePolicy" variant="default">{{
              $t("common.save")
            }}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </template>
  </CrudPageLayout>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { useAccessControlStore } from "@/store/accessControlStore";
import type { AccessControlRule } from "@/store/types";
import { useProviderStore } from "@/store/providerStore";
import { normalizeError } from "@/lib/error";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import { Plus, Edit2, Trash2, ShieldAlert, Loader2 } from "lucide-vue-next";
import { confirm } from "@/lib/confirmController";
import { toastController } from "@/lib/toastController";

const { t: $t } = useI18n();
const store = useAccessControlStore();
const providerStore = useProviderStore();

// UI States
const storeLoading = ref(true);
const providersLoading = ref(true);
const showEditModal = ref(false);
const pageError = ref<string | null>(null);

const setShowEditModal = (val: boolean) => {
  showEditModal.value = val;
};

const newRuleTemplate = () => ({
  id: null as number | null,
  rule_type: "ALLOW",
  priority: 0,
  scope: "PROVIDER",
  provider_id: null as string | null,
  model_id: null as string | null,
  description: "",
  is_enabled: true,
});

type EditingRule = ReturnType<typeof newRuleTemplate>;

const newPolicyTemplate = () => ({
  id: null as number | null,
  name: "",
  default_action: "ALLOW" as "ALLOW" | "DENY",
  description: "",
  rules: [] as EditingRule[],
});

const editingPolicy = ref(newPolicyTemplate());

// Options
const defaultActionOptions = computed(() =>
  ["ALLOW", "DENY"].map((o) => ({
    value: o,
    label: $t(`accessControlPage.modal.option${o}`),
  })),
);
const ruleTypeOptions = computed(() =>
  ["ALLOW", "DENY"].map((o) => ({
    value: o,
    label: $t(`accessControlPage.modal.option${o}`),
  })),
);
const scopeOptions = computed(() =>
  ["PROVIDER", "MODEL"].map((o) => ({
    value: o,
    label: $t(`accessControlPage.rules.scope${o}`),
  })),
);
const providerOptions = computed(() =>
  providerStore.providers.map((p) => ({
    value: p.provider.id,
    label: p.provider.name,
  })),
);

const getModelOptions = (providerIdStr: string | null) => {
  const providerId = Number(providerIdStr);
  if (!providerId || isNaN(providerId)) return [];
  const pDetail = providerStore.providers.find((p) => p.provider.id === providerId);
  return (pDetail?.models || []).map((m) => ({
    value: m.model.id,
    label: m.model.model_name,
  }));
};

const onScopeChange = (rule: EditingRule) => {
  if (rule.scope === "PROVIDER") rule.model_id = null;
};

const onProviderChange = (rule: EditingRule) => {
  rule.model_id = null;
};

// Methods
const fetchPolicyDetailAPI = async (id: number) => {
  try {
    const response = await Api.getAccessControlDetail(id);
    return response;
  } catch (error) {
    console.error("Failed to fetch policy detail:", error);
    return null;
  }
};

const handleOpenAddModal = () => {
  editingPolicy.value = newPolicyTemplate();
  showEditModal.value = true;
};

const handleOpenEditModal = async (id: number) => {
  const detail = await fetchPolicyDetailAPI(id);
  if (detail) {
    editingPolicy.value = {
      id: detail.id,
      name: detail.name,
      default_action: detail.default_action.toUpperCase() as "ALLOW" | "DENY",
      description: detail.description || "",
      rules: (detail.rules || [])
        .map((r: AccessControlRule) => ({
          id: r.id,
          rule_type: r.rule_type.toUpperCase(),
          priority: r.priority,
          scope: r.scope.toUpperCase(),
          provider_id: r.provider_id ? String(r.provider_id) : null,
          model_id: r.model_id ? String(r.model_id) : null,
          description: r.description || "",
          is_enabled: r.is_enabled,
        }))
        .sort((a, b) => a.priority - b.priority),
    };
    showEditModal.value = true;
  } else {
    toastController.error($t("accessControlPage.alert.loadDetailFailed"));
  }
};

const handleCloseModal = () => {
  showEditModal.value = false;
};

const handleDeletePolicy = async (id: number, name: string) => {
  if (
    await confirm({
      title: $t("accessControlPage.confirmDelete", { name }),
    })
  ) {
    try {
      await Api.deleteAccessControl(id);
      await store.fetchPolicies();
    } catch (error: unknown) {
      toastController.error(
        $t("accessControlPage.alert.deleteFailed", {
          error: (error as Error).message || "Unknown Error",
        }),
      );
    }
  }
};

const handleSavePolicy = async () => {
  if (!editingPolicy.value.name) {
    toastController.error($t("accessControlPage.alert.nameRequired"));
    return;
  }

  const payload = {
    name: editingPolicy.value.name,
    default_action: editingPolicy.value.default_action,
    description: editingPolicy.value.description || null,
    rules: editingPolicy.value.rules.map((rule) => ({
      rule_type: rule.rule_type,
      priority: Number(rule.priority) || 0,
      scope: rule.scope,
      provider_id: rule.provider_id ? Number(rule.provider_id) : null,
      model_id:
        rule.scope === "MODEL" && rule.model_id ? Number(rule.model_id) : null,
      description: rule.description || null,
      is_enabled: rule.is_enabled,
    })),
  };

  try {
    if (editingPolicy.value.id) {
      await Api.updateAccessControl(editingPolicy.value.id, payload);
    } else {
      await Api.createAccessControl(payload);
    }
    showEditModal.value = false;
    await store.fetchPolicies();
  } catch (error: unknown) {
    toastController.error(
      $t("accessControlPage.alert.saveFailed", {
        error: (error as Error).message || "Unknown Error",
      }),
    );
  }
};

const addRule = () => {
  editingPolicy.value.rules.push(newRuleTemplate());
};

const removeRule = (index: number) => {
  editingPolicy.value.rules.splice(index, 1);
};

onMounted(async () => {
  pageError.value = null;

  storeLoading.value = true;
  try {
    await store.fetchPolicies();
  } catch (error: unknown) {
    pageError.value = normalizeError(error, "Unknown Error").message;
  } finally {
    storeLoading.value = false;
  }

  providersLoading.value = true;
  try {
    await providerStore.fetchProviders();
  } catch (error: unknown) {
    pageError.value = pageError.value || normalizeError(error, "Unknown Error").message;
  } finally {
    providersLoading.value = false;
  }
});
</script>
