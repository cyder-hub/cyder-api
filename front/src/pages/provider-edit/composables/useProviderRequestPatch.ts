import type { Ref } from "vue";

import * as requestPatchService from "@/services/requestPatch";
import type {
  RequestPatchPayload,
  RequestPatchUpdatePayload,
} from "@/services/types";
import type { RequestPatchRuleActions } from "@/components/request-patch/types";
import type { EditingProviderData } from "../types";

export function useProviderRequestPatch(
  editingData: Ref<EditingProviderData>,
) {
  const refreshRules = async () => {
    if (!editingData.value.id) return;

    const rules = await requestPatchService.listProviderRequestPatches(
      editingData.value.id,
    );
    editingData.value.request_patches = rules;
  };

  const actions: RequestPatchRuleActions = {
    createRule: (payload: RequestPatchPayload) => {
      return requestPatchService.createProviderRequestPatch(
        editingData.value.id!,
        payload,
      );
    },
    updateRule: (ruleId: number, payload: RequestPatchUpdatePayload) => {
      return requestPatchService.updateProviderRequestPatch(
        editingData.value.id!,
        ruleId,
        payload,
      );
    },
    deleteRule: (ruleId: number) => {
      return requestPatchService.deleteProviderRequestPatch(
        editingData.value.id!,
        ruleId,
      );
    },
  };

  return {
    actions,
    refreshRules,
  };
}
