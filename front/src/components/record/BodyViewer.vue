<template>
  <div v-if="isLoadingBodies" class="py-4 text-center text-sm text-gray-500">
    {{ $t("recordPage.detailDialog.payloadViewer.loadingBodies") }}
  </div>
  <div
    v-else-if="bodyError"
    class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
  >
    {{ bodyError }}
  </div>
  <div v-else-if="bundleView" class="space-y-4">
    <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <h4 class="text-sm font-semibold text-gray-900">
          {{ $t("recordPage.detailDialog.payloadViewer.title") }}
        </h4>
        <p class="mt-1 text-xs text-gray-500">
          {{ $t("recordPage.detailDialog.payloadViewer.description") }}
        </p>
      </div>
      <Badge variant="outline" class="font-mono text-xs">
        V{{ bundleView.version }}
      </Badge>
    </div>

    <template v-if="legacyBodies">
      <div
        v-if="
          legacyBodies.userRequestBody !== legacyBodies.llmRequestBody &&
          legacyBodies.userRequestBody &&
          legacyBodies.llmRequestBody
        "
        class="grid grid-cols-1 gap-4 md:grid-cols-2"
      >
        <SingleRequestBodyContent
          :content="legacyBodies.userRequestBody"
          :title="$t('recordPage.detailDialog.payloadViewer.userRequestBody')"
        />
        <SingleRequestBodyContent :content="legacyLlmContent" :title="legacyLlmTitle">
          <template v-if="legacyPatchInfo.isPatch" #action>
            <Button
              size="sm"
              variant="ghost"
              class="h-8 px-2 text-[11px]"
              @click="showLegacyPatched = !showLegacyPatched"
            >
              {{
                showLegacyPatched
                  ? $t("recordPage.detailDialog.payloadViewer.showRawPatch")
                  : $t("recordPage.detailDialog.payloadViewer.showPatchedBody")
              }}
            </Button>
          </template>
        </SingleRequestBodyContent>
      </div>
      <SingleRequestBodyContent
        v-else
        :content="legacyBodies.userRequestBody || legacyBodies.llmRequestBody"
        :title="$t('recordPage.detailDialog.payloadViewer.requestBody')"
      />

      <div
        v-if="
          legacyBodies.userResponseBody !== legacyBodies.llmResponseBody &&
          legacyBodies.userResponseBody &&
          legacyBodies.llmResponseBody
        "
        class="grid grid-cols-1 gap-4 md:grid-cols-2"
      >
        <SingleResponseBodyContent
          :content="legacyBodies.llmResponseBody"
          :title="$t('recordPage.detailDialog.payloadViewer.llmResponseBody')"
          :status="status"
        />
        <SingleResponseBodyContent
          :content="legacyBodies.userResponseBody"
          :title="$t('recordPage.detailDialog.payloadViewer.userResponseBody')"
          :status="status"
        />
      </div>
      <SingleResponseBodyContent
        v-else
        :content="legacyBodies.userResponseBody || legacyBodies.llmResponseBody"
        :title="$t('recordPage.detailDialog.payloadViewer.responseBody')"
        :status="status"
      />
    </template>

    <template v-else-if="v2Bodies">
      <section class="space-y-3">
        <div class="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
          <h4 class="text-sm font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.payloadViewer.userExchange") }}
          </h4>
          <span class="text-xs text-gray-500">
            {{
              $t("recordPage.detailDialog.payloadViewer.responseCapture", {
                state: formatCaptureState(v2Bodies.request.userResponseCaptureState),
              })
            }}
          </span>
        </div>
        <div
          v-if="hasRequestLevelPayload"
          class="grid grid-cols-1 gap-4 md:grid-cols-2"
        >
          <SingleRequestBodyContent
            v-if="v2Bodies.request.userRequestBody"
            :content="v2Bodies.request.userRequestBody"
            :title="$t('recordPage.detailDialog.payloadViewer.userRequestBody')"
          />
          <SingleResponseBodyContent
            v-if="v2Bodies.request.userResponseBody"
            :content="v2Bodies.request.userResponseBody"
            :title="$t('recordPage.detailDialog.payloadViewer.userResponseBody')"
            :status="status"
          />
        </div>
        <div
          v-else
          class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-5 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.payloadViewer.noRequestLevelPayload") }}
        </div>
      </section>

      <section class="space-y-3 border-t border-gray-100 pt-4">
        <div class="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
          <h4 class="text-sm font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.payloadViewer.attemptPayloads") }}
          </h4>
          <Badge variant="outline" class="font-mono text-xs">
            {{ $t("recordPage.detailDialog.payloadViewer.attemptCount", { count: v2AttemptRows.length }) }}
          </Badge>
        </div>

        <div
          v-if="v2AttemptRows.length === 0"
          class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-5 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.payloadViewer.noAttemptPayloads") }}
        </div>

        <div
          v-for="attempt in v2AttemptRows"
          :key="attempt.key"
          class="space-y-3 border-t border-gray-100 pt-4 first:border-t-0 first:pt-0"
        >
          <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0 space-y-1">
              <div class="flex flex-wrap items-center gap-2">
                <span class="text-sm font-semibold text-gray-900">
                  {{ $t("recordPage.detailDialog.payloadViewer.attemptTitle", { index: attempt.attemptIndex }) }}
                </span>
                <Badge :variant="getStatusBadgeVariant(attempt.status)">
                  {{ attempt.status || $t("recordPage.detailDialog.payloadViewer.unknownStatus") }}
                </Badge>
                <Badge variant="outline" class="font-mono text-[11px]">
                  {{ attempt.schedulerAction || $t("recordPage.detailDialog.payloadViewer.noAction") }}
                </Badge>
                <Badge v-if="attempt.httpStatus != null" variant="outline" class="font-mono text-[11px]">
                  HTTP {{ attempt.httpStatus }}
                </Badge>
              </div>
              <p class="break-all font-mono text-xs text-gray-600">
                {{ attempt.providerModelDisplay }}
              </p>
            </div>
            <div class="font-mono text-[11px] text-gray-500 sm:text-right">
              <div>
                {{ $t("recordPage.detailDialog.payloadViewer.requestBlob", { value: formatNullableNumber(attempt.requestBlobId) }) }}
              </div>
              <div>
                {{ $t("recordPage.detailDialog.payloadViewer.patchBlob", { value: formatNullableNumber(attempt.requestPatchId) }) }}
              </div>
              <div>
                {{ $t("recordPage.detailDialog.payloadViewer.responseBlob", { value: formatNullableNumber(attempt.responseBlobId) }) }}
              </div>
            </div>
          </div>

          <div
            v-if="attempt.requestContent || attempt.responseContent"
            class="grid grid-cols-1 gap-4 md:grid-cols-2"
          >
            <SingleRequestBodyContent
              v-if="attempt.requestContent"
              :content="displayAttemptRequestContent(attempt)"
              :title="attemptRequestTitle(attempt)"
            >
              <template v-if="attempt.requestRawPatchContent" #action>
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-8 px-2 text-[11px]"
                  @click="toggleAttemptPatch(attempt.key)"
                >
                  {{
                    isAttemptPatchRaw(attempt.key)
                      ? $t("recordPage.detailDialog.payloadViewer.showPatchedBody")
                      : $t("recordPage.detailDialog.payloadViewer.showRawPatch")
                  }}
                </Button>
              </template>
            </SingleRequestBodyContent>
            <SingleResponseBodyContent
              v-if="attempt.responseContent"
              :content="attempt.responseContent"
              :title="$t('recordPage.detailDialog.payloadViewer.llmResponseBody')"
              :status="attempt.status"
            />
          </div>
          <div
            v-else
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-5 text-sm text-gray-500"
          >
            {{ $t("recordPage.detailDialog.payloadViewer.noAttemptBody") }}
            {{
              $t("recordPage.detailDialog.payloadViewer.responseCaptureSentence", {
                state: formatCaptureState(attempt.responseCaptureState),
              })
            }}
          </div>
          <p v-if="attempt.requestPatchError" class="text-xs text-amber-700">
            {{ formatPatchError(attempt.requestPatchError) }}
          </p>
        </div>
      </section>
    </template>
  </div>
  <div
    v-else
    class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-5 text-sm text-gray-500"
  >
    {{ $t("recordPage.detailDialog.payloadViewer.bundleEmpty") }}
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import * as msgpack from "@msgpack/msgpack";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { RecordAttempt } from "@/store/types";
import {
  buildLegacyPatchInfo,
  buildV2AttemptRows,
  decodeBundleView,
  type BundleView,
  type V2AttemptRow,
} from "./bodyBundleView";
import SingleRequestBodyContent from "./SingleRequestBodyContent.vue";
import SingleResponseBodyContent from "./SingleResponseBodyContent.vue";

const props = defineProps<{
  recordId: number;
  storageType: string | null;
  status: string | null;
  attempts?: RecordAttempt[];
}>();

const bundleView = ref<BundleView | null>(null);
const isLoadingBodies = ref(false);
const bodyError = ref<string | null>(null);
const showLegacyPatched = ref(true);
const rawPatchByAttempt = ref<Record<string, boolean>>({});
const { t: $t } = useI18n();

const fetchAndDecodeBody = async () => {
  if (!props.storageType || !props.recordId) return;
  isLoadingBodies.value = true;
  bodyError.value = null;
  bundleView.value = null;
  rawPatchByAttempt.value = {};
  showLegacyPatched.value = true;

  try {
    const buffer = await Api.getRecordContent(props.recordId);
    const decoded = msgpack.decode(new Uint8Array(buffer)) as Record<string, unknown>;
    bundleView.value = decodeBundleView(decoded);
  } catch (error) {
    console.error("Failed to fetch or decode body content:", error);
    bodyError.value = $t("recordPage.detailDialog.payloadViewer.fetchError", {
      error: error instanceof Error ? error.message : String(error),
    });
  } finally {
    isLoadingBodies.value = false;
  }
};

const legacyBodies = computed(() =>
  bundleView.value?.kind === "legacy" ? bundleView.value : null,
);

const v2Bodies = computed(() =>
  bundleView.value?.kind === "v2" ? bundleView.value : null,
);

const hasRequestLevelPayload = computed(
  () =>
    Boolean(v2Bodies.value?.request.userRequestBody) ||
    Boolean(v2Bodies.value?.request.userResponseBody),
);

const v2AttemptRows = computed<V2AttemptRow[]>(() => {
  return buildV2AttemptRows(v2Bodies.value?.attempts ?? [], props.attempts ?? [], {
    unknownProvider: $t("recordPage.detailDialog.payloadViewer.unknownProvider"),
    unknownModel: $t("recordPage.detailDialog.payloadViewer.unknownModel"),
  });
});

const legacyPatchInfo = computed(() => {
  return buildLegacyPatchInfo(legacyBodies.value);
});

const legacyLlmContent = computed(() => {
  return legacyPatchInfo.value.isPatch && showLegacyPatched.value
    ? legacyPatchInfo.value.patchedContent
    : legacyBodies.value?.llmRequestBody ?? null;
});

const legacyLlmTitle = computed(() => {
  if (!legacyPatchInfo.value.isPatch) {
    return $t("recordPage.detailDialog.payloadViewer.llmRequestBody");
  }
  return showLegacyPatched.value
    ? $t("recordPage.detailDialog.payloadViewer.llmRequestBodyPatched")
    : $t("recordPage.detailDialog.payloadViewer.llmRequestBodyRawPatch");
});

const getStatusBadgeVariant = (status: string | null) => {
  switch (status) {
    case "SUCCESS":
      return "default";
    case "ERROR":
      return "destructive";
    case "CANCELLED":
    case "SKIPPED":
      return "secondary";
    default:
      return "outline";
  }
};

const formatNullableNumber = (value: number | null) => value ?? "/";

const formatCaptureState = (state: string | null | undefined) =>
  state ? state.replaceAll("_", " ") : "/";

const formatPatchError = (error: string) => {
  const prefix = "Unable to apply JSON patch: ";
  if (error === "Unable to apply JSON patch because the target blob is missing.") {
    return $t("recordPage.detailDialog.payloadViewer.patchMissing");
  }
  if (error.startsWith(prefix)) {
    return $t("recordPage.detailDialog.payloadViewer.patchFailed", {
      error: error.slice(prefix.length),
    });
  }
  return error;
};

const isAttemptPatchRaw = (key: string) => Boolean(rawPatchByAttempt.value[key]);

const toggleAttemptPatch = (key: string) => {
  rawPatchByAttempt.value = {
    ...rawPatchByAttempt.value,
    [key]: !rawPatchByAttempt.value[key],
  };
};

const displayAttemptRequestContent = (attempt: V2AttemptRow) => {
  if (isAttemptPatchRaw(attempt.key) && attempt.requestRawPatchContent) {
    return attempt.requestRawPatchContent;
  }
  return attempt.requestContent;
};

const attemptRequestTitle = (attempt: V2AttemptRow) => {
  if (!attempt.requestRawPatchContent) {
    return $t("recordPage.detailDialog.payloadViewer.llmRequestBody");
  }
  return isAttemptPatchRaw(attempt.key)
    ? $t("recordPage.detailDialog.payloadViewer.llmRequestBodyRawPatch")
    : $t("recordPage.detailDialog.payloadViewer.llmRequestBodyPatched");
};

onMounted(fetchAndDecodeBody);

watch(
  () => [props.recordId, props.storageType],
  () => {
    void fetchAndDecodeBody();
  },
);
</script>
