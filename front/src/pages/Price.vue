<template>
  <CrudPageLayout
    :title="$t('pricePage.title')"
    :description="$t('pricePage.description', 'Manage billing plans and rules')"
  >

    <!-- Billing Plans Section -->
    <div class="space-y-4">
      <div class="flex justify-between items-center">
        <h2 class="text-lg font-medium text-gray-900">
          {{ $t("pricePage.plans.title") }}
        </h2>
        <Button variant="outline" @click="openNewPlanModal">
          <Plus class="h-4 w-4 mr-1.5" />
          {{ $t("pricePage.plans.add") }}
        </Button>
      </div>

      <div
        v-if="loadingPlans"
        class="flex items-center justify-center py-16 text-gray-500"
      >
        {{ $t("pricePage.plans.loading") }}
      </div>
      <div v-else class="border border-gray-200 rounded-lg overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.plans.table.name") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.plans.table.description") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.plans.modal.currency") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("pricePage.plans.table.actions") }}</TableHead
              >
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="plan in priceStore.billingPlans"
              :key="plan.id"
              class="cursor-pointer transition-colors"
              :class="{
                'bg-gray-100 font-medium':
                  priceStore.selectedPlanId === plan.id,
              }"
              @click="handleSelectPlan(plan.id)"
            >
              <TableCell>{{ plan.name }}</TableCell>
              <TableCell>{{ plan.description }}</TableCell>
              <TableCell>{{ plan.currency }}</TableCell>
              <TableCell class="text-right">
                <Button
                  variant="ghost"
                  size="sm"
                  @click.stop="openEditPlanModal(plan)"
                >
                  <Edit class="h-3.5 w-3.5 mr-1" />
                  {{ $t("common.edit") }}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="text-gray-400 hover:text-red-600"
                  @click.stop="handleDeletePlan(plan.id, plan.name)"
                >
                  <Trash2 class="h-3.5 w-3.5 mr-1" />
                  {{ $t("common.delete") }}
                </Button>
              </TableCell>
            </TableRow>
            <TableRow v-if="!priceStore.billingPlans.length">
              <TableCell
                colspan="4"
                class="text-center py-20 text-sm font-medium text-gray-500"
                >{{ $t("common.noData", "No data") }}</TableCell
              >
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </div>

    <!-- Price Rules Section -->
    <div
      v-if="priceStore.selectedPlanId"
      class="space-y-4 pt-6 mt-6 border-t border-gray-100"
    >
      <div class="flex justify-between items-center">
        <h2 class="text-lg font-medium text-gray-900">
          {{ $t("pricePage.rules.title") }}
        </h2>
        <Button variant="outline" @click="openNewRuleModal">
          <Plus class="h-4 w-4 mr-1.5" />
          {{ $t("pricePage.rules.add") }}
        </Button>
      </div>

      <div
        v-if="loadingRules"
        class="flex items-center justify-center py-16 text-gray-500"
      >
        {{ $t("pricePage.rules.loading") }}
      </div>
      <div v-else class="border border-gray-200 rounded-lg overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.rules.table.description") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.rules.table.enabled") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.rules.table.usageType") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.rules.table.mediaType") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("pricePage.rules.table.price") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("pricePage.rules.table.effectiveFrom") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("pricePage.rules.table.actions") }}</TableHead
              >
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-for="rule in priceStore.priceRules" :key="rule.id">
              <TableCell>{{ rule.description }}</TableCell>
              <TableCell>
                <Badge
                  :variant="rule.is_enabled ? 'secondary' : 'outline'"
                  class="font-mono text-xs"
                >
                  {{ rule.is_enabled ? $t("common.yes") : $t("common.no") }}
                </Badge>
              </TableCell>
              <TableCell
                ><Badge variant="outline" class="text-xs">{{
                  rule.usage_type
                }}</Badge></TableCell
              >
              <TableCell
                ><Badge variant="outline" class="text-xs">{{
                  rule.media_type
                }}</Badge></TableCell
              >
              <TableCell class="text-right"
                >{{ rule.price_in_micro_units / 1000 }}
                {{ selectedPlan?.currency }}</TableCell
              >
              <TableCell>{{ formatTimestamp(rule.effective_from) }}</TableCell>
              <TableCell class="text-right">
                <Button
                  variant="ghost"
                  size="sm"
                  @click="openEditRuleModal(rule)"
                >
                  <Edit class="h-3.5 w-3.5 mr-1" />
                  {{ $t("common.edit") }}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="text-gray-400 hover:text-red-600"
                  @click="handleDeleteRule(rule.id)"
                >
                  <Trash2 class="h-3.5 w-3.5 mr-1" />
                  {{ $t("common.delete") }}
                </Button>
              </TableCell>
            </TableRow>
            <TableRow v-if="!priceStore.priceRules.length">
              <TableCell
                colspan="7"
                class="text-center py-20 text-sm font-medium text-gray-500"
                >{{ $t("common.noData", "No data") }}</TableCell
              >
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </div>

    <template #modals>
      <!-- Billing Plan Modal -->
      <Dialog :open="isPlanModalOpen" @update:open="setIsPlanModalOpen">
      <DialogContent class="max-w-lg">
        <DialogHeader>
          <DialogTitle class="text-lg font-semibold text-gray-900">{{
            editingPlan.id
              ? $t("pricePage.plans.modal.titleEdit")
              : $t("pricePage.plans.modal.titleAdd")
          }}</DialogTitle>
        </DialogHeader>
        <div class="space-y-4">
          <div class="space-y-1.5">
            <Label class="text-gray-700"
              >{{ $t("pricePage.plans.modal.name") }}
              <span class="text-red-500 ml-0.5">*</span></Label
            >
            <Input v-model="editingPlan.name" class="font-mono text-sm" />
          </div>
          <div class="space-y-1.5">
            <Label class="text-gray-700">{{
              $t("pricePage.plans.modal.description")
            }}</Label>
            <Input v-model="editingPlan.description" />
          </div>
          <div class="space-y-1.5">
            <Label class="text-gray-700"
              >{{ $t("pricePage.plans.modal.currency") }}
              <span class="text-red-500 ml-0.5">*</span></Label
            >
            <Select v-model="editingPlan.currency">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="USD">USD</SelectItem>
                <SelectItem value="CNY">CNY</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
        <DialogFooter class="border-t border-gray-100 pt-4 mt-2">
          <Button
            variant="ghost"
            class="text-gray-600"
            @click="setIsPlanModalOpen(false)"
            >{{ $t("common.cancel") }}</Button
          >
          <Button variant="default" @click="handleSavePlan">{{
            $t("common.save")
          }}</Button>
        </DialogFooter>
      </DialogContent>
      </Dialog>

      <!-- Price Rule Modal -->
      <Dialog :open="isRuleModalOpen" @update:open="setIsRuleModalOpen">
      <DialogContent class="max-w-4xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle class="text-lg font-semibold text-gray-900">{{
            editingRule.id
              ? $t("pricePage.rules.modal.titleEdit")
              : $t("pricePage.rules.modal.titleAdd")
          }}</DialogTitle>
        </DialogHeader>
        <div class="space-y-4">
          <div class="space-y-1.5">
            <Label class="text-gray-700">{{
              $t("pricePage.rules.modal.description")
            }}</Label>
            <Input v-model="editingRule.description" />
          </div>
          <div
            class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
          >
            <Label
              for="is_enabled_rule_checkbox"
              class="cursor-pointer text-gray-700 font-medium"
              >{{ $t("pricePage.rules.modal.enabled") }}</Label
            >
            <Checkbox
              id="is_enabled_rule_checkbox"
              :checked="editingRule.is_enabled"
              @update:checked="(v: boolean) => (editingRule.is_enabled = v)"
            />
          </div>
          <div class="grid grid-cols-2 gap-4">
            <div class="space-y-1.5">
              <Label class="text-gray-700"
                >{{ $t("pricePage.rules.modal.usageType") }}
                <span class="text-red-500 ml-0.5">*</span></Label
              >
              <Select v-model="editingRule.usage_type">
                <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="type in USAGE_TYPES"
                    :key="type"
                    :value="type"
                    >{{ type }}</SelectItem
                  >
                </SelectContent>
              </Select>
            </div>
            <div class="space-y-1.5">
              <Label class="text-gray-700">{{
                $t("pricePage.rules.modal.mediaType")
              }}</Label>
              <Select v-model="editingRule.media_type">
                <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="type in MEDIA_TYPES"
                    :key="type.value"
                    :value="type.value"
                    >{{ type.label }}</SelectItem
                  >
                </SelectContent>
              </Select>
            </div>
          </div>
          <div class="space-y-1.5">
            <Label class="text-gray-700"
              >{{ $t("pricePage.rules.modal.price") }}
              <span class="text-red-500 ml-0.5">*</span></Label
            >
            <Input
              type="number"
              v-model.number="editingRule.price_in_micro_units"
              step="any"
              class="font-mono text-sm"
            />
          </div>
          <div class="grid grid-cols-2 gap-4">
            <div class="space-y-1.5">
              <Label class="text-gray-700"
                >{{ $t("pricePage.rules.modal.effectiveFrom") }}
                <span class="text-red-500 ml-0.5">*</span></Label
              >
              <Input
                type="datetime-local"
                :model-value="toDateTimeLocal(editingRule.effective_from)"
                @update:model-value="
                  (v) =>
                    (editingRule.effective_from = fromDateTimeLocal(
                      v as string,
                    ))
                "
              />
            </div>
            <div class="space-y-1.5">
              <Label class="text-gray-700">{{
                $t("pricePage.rules.modal.effectiveUntil")
              }}</Label>
              <Input
                type="datetime-local"
                :model-value="toDateTimeLocal(editingRule.effective_until)"
                @update:model-value="
                  (v) =>
                    (editingRule.effective_until = fromDateTimeLocal(
                      v as string,
                    ))
                "
              />
            </div>
          </div>
        </div>
        <DialogFooter class="border-t border-gray-100 pt-4 mt-2">
          <Button
            variant="ghost"
            class="text-gray-600"
            @click="setIsRuleModalOpen(false)"
            >{{ $t("common.cancel") }}</Button
          >
          <Button variant="default" @click="handleSaveRule">{{
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
import { usePriceStore } from "@/store/priceStore";
import type { BillingPlan, PriceRule } from "@/store/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import { Plus, Edit, Trash2 } from "lucide-vue-next";
import { confirm } from "@/lib/confirmController";
import { toastController } from "@/lib/toastController";
import { formatTimestamp } from "@/lib/utils";

const { t: $t } = useI18n();
const priceStore = usePriceStore();

const loadingPlans = ref(true);
const loadingRules = ref(false); // Can be tied to a watch if needed

onMounted(async () => {
  loadingPlans.value = true;
  await priceStore.fetchBillingPlans();
  loadingPlans.value = false;
});

const selectedPlan = computed(() =>
  priceStore.billingPlans.find((p) => p.id === priceStore.selectedPlanId),
);

const handleSelectPlan = (planId: number) => {
  priceStore.setSelectedPlanId(planId);
};

// --- Billing Plans ---
const isPlanModalOpen = ref(false);
const setIsPlanModalOpen = (val: boolean) => {
  isPlanModalOpen.value = val;
};

const editingPlan = ref<{
  id?: number;
  name?: string;
  description?: string;
  currency?: string;
}>({});

const openNewPlanModal = () => {
  editingPlan.value = {
    id: undefined,
    name: "",
    description: "",
    currency: "USD",
  };
  isPlanModalOpen.value = true;
};

const openEditPlanModal = (plan: BillingPlan) => {
  editingPlan.value = {
    id: plan.id,
    name: plan.name,
    description: plan.description || "",
    currency: plan.currency,
  };
  isPlanModalOpen.value = true;
};

const handleSavePlan = async () => {
  const plan = editingPlan.value;
  if (!plan.name) {
    toastController.error($t("pricePage.alert.planNameRequired"));
    return;
  }

  const payload = {
    name: plan.name,
    description: plan.description,
    currency: plan.currency,
  };

  try {
    if (plan.id) {
      await Api.updateBillingPlan(plan.id, payload);
    } else {
      await Api.createBillingPlan(payload);
    }
    isPlanModalOpen.value = false;
    await priceStore.fetchBillingPlans();
  } catch (error: any) {
    toastController.error(
      $t("pricePage.alert.planSaveFailed", {
        error: error.message || $t("common.unknownError"),
      }),
    );
  }
};

const handleDeletePlan = async (planId: number, planName: string) => {
  if (
    await confirm({
      title: $t("pricePage.confirmDeletePlan", { name: planName }),
    })
  ) {
    try {
      await Api.deleteBillingPlan(planId);
      await priceStore.fetchBillingPlans();
      if (priceStore.selectedPlanId === planId) {
        priceStore.setSelectedPlanId(null);
      }
    } catch (error: any) {
      toastController.error(
        $t("pricePage.alert.planDeleteFailed", {
          error: error.message || $t("common.unknownError"),
        }),
      );
    }
  }
};

// --- Price Rules ---
const isRuleModalOpen = ref(false);
const setIsRuleModalOpen = (val: boolean) => {
  isRuleModalOpen.value = val;
};

const editingRule = ref<{
  id?: number;
  plan_id?: number;
  description?: string;
  is_enabled?: boolean;
  effective_from?: number;
  effective_until?: number;
  usage_type?: string;
  media_type?: string;
  price_in_micro_units?: number;
}>({});

const openNewRuleModal = () => {
  if (!priceStore.selectedPlanId) {
    toastController.error($t("pricePage.alert.selectPlanFirst"));
    return;
  }
  editingRule.value = {
    id: undefined,
    plan_id: priceStore.selectedPlanId,
    description: "",
    is_enabled: true,
    effective_from: Date.now(),
    effective_until: undefined,
    usage_type: "COMPLETION",
    media_type: "UNSET",
    price_in_micro_units: 0,
  };
  isRuleModalOpen.value = true;
};

const openEditRuleModal = (rule: PriceRule) => {
  editingRule.value = {
    id: rule.id,
    plan_id: rule.plan_id,
    description: rule.description || "",
    is_enabled: rule.is_enabled,
    effective_from: rule.effective_from,
    effective_until: rule.effective_until || undefined,
    usage_type: rule.usage_type,
    price_in_micro_units: rule.price_in_micro_units / 1000,
    media_type: rule.media_type || "UNSET",
  };
  isRuleModalOpen.value = true;
};

const handleSaveRule = async () => {
  const rule = editingRule.value;
  const price = Number(rule.price_in_micro_units);
  const payload = {
    ...rule,
    media_type: rule.media_type === "UNSET" ? null : rule.media_type,
    price_in_micro_units: isNaN(price) ? 0 : Math.round(price * 1000),
  };

  try {
    if (rule.id) {
      await Api.updatePriceRule(rule.id, payload);
    } else {
      await Api.createPriceRule(payload);
    }
    isRuleModalOpen.value = false;
    if (priceStore.selectedPlanId !== null) {
      await priceStore.fetchPriceRules(priceStore.selectedPlanId);
    }
  } catch (error: any) {
    toastController.error(
      $t("pricePage.alert.ruleSaveFailed", {
        error: error.message || $t("common.unknownError"),
      }),
    );
  }
};

const handleDeleteRule = async (ruleId: number) => {
  if (
    await confirm({
      title: $t("pricePage.confirmDeleteRule"),
    })
  ) {
    try {
      await Api.deletePriceRule(ruleId);
      if (priceStore.selectedPlanId !== null) {
        await priceStore.fetchPriceRules(priceStore.selectedPlanId);
      }
    } catch (error: any) {
      toastController.error(
        $t("pricePage.alert.ruleDeleteFailed", {
          error: error.message || $t("common.unknownError"),
        }),
      );
    }
  }
};

// --- Constants & Helpers ---
const USAGE_TYPES = ["PROMPT", "COMPLETION", "INVOCATION"];
const MEDIA_TYPES = [
  "",
  "IMAGE",
  "AUDIO",
  "VIDEO",
  "CACHE_TEXT",
  "CACHE_AUDIO",
  "CACHE_VIDEO",
].map((mt) => ({
  value: mt || "UNSET",
  label: mt || $t("pricePage.rules.modal.mediaTypeDefault"),
}));
const toDateTimeLocal = (ms: number | null | undefined): string => {
  if (!ms) return "";
  const date = new Date(ms);
  const timezoneOffset = date.getTimezoneOffset() * 60000;
  return new Date(date.getTime() - timezoneOffset).toISOString().slice(0, 16);
};

const fromDateTimeLocal = (str: string): number | undefined => {
  return str ? new Date(str).getTime() : undefined;
};
</script>
