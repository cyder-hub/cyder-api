import { defineStore } from "pinia";
import { ref, watch } from "vue";
import { Api } from "@/services/request";
import type { BillingPlan, PriceRule } from "./types";

export const usePriceStore = defineStore("price", () => {
  const billingPlans = ref<BillingPlan[]>([]);
  const priceRules = ref<PriceRule[]>([]);
  const selectedPlanId = ref<number | null>(null);

  async function fetchBillingPlans() {
    try {
      billingPlans.value = await Api.getBillingPlanList();
    } catch (error) {
      console.error("Failed to fetch billing plans", error);
      billingPlans.value = [];
    }
  }

  async function fetchPriceRules(planId: number) {
    if (!planId) {
      priceRules.value = [];
      return;
    }
    try {
      priceRules.value = await Api.getPriceRuleListByPlan(planId);
    } catch (error) {
      console.error(`Failed to fetch price rules for plan ${planId}`, error);
      priceRules.value = [];
    }
  }

  function setSelectedPlanId(planId: number | null) {
    selectedPlanId.value = planId;
  }

  watch(selectedPlanId, (newPlanId) => {
    if (newPlanId !== null) {
      fetchPriceRules(newPlanId);
    } else {
      priceRules.value = [];
    }
  });

  const refetchBillingPlans = fetchBillingPlans;
  const loadBillingPlans = fetchBillingPlans;
  const refetchPriceRules = () => {
    if (selectedPlanId.value !== null) {
      fetchPriceRules(selectedPlanId.value);
    }
  };

  return {
    billingPlans,
    priceRules,
    selectedPlanId,
    fetchBillingPlans,
    fetchPriceRules,
    setSelectedPlanId,
    refetchBillingPlans,
    loadBillingPlans,
    refetchPriceRules,
  };
});
