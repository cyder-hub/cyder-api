<template>
  <div v-if="isLoadingBodies" class="py-4 text-center text-sm text-gray-500">
    Loading bodies...
  </div>
  <div v-else-if="bodies" class="space-y-4">
    <div
      v-if="
        bodies.user_request_body !== bodies.llm_request_body &&
        bodies.user_request_body &&
        bodies.llm_request_body
      "
      class="grid grid-cols-1 gap-4 md:grid-cols-2"
    >
      <SingleRequestBodyContent
        :content="bodies.user_request_body"
        title="User Request Body"
      />
      <SingleRequestBodyContent :content="llmContent" :title="llmTitle">
        <template v-if="patchInfo.isPatch" #action>
          <Button
            size="sm"
            variant="ghost"
            class="h-8 px-2 text-[11px]"
            @click="showPatched = !showPatched"
          >
            {{ showPatched ? "Show Raw Patch" : "Show Patched Body" }}
          </Button>
        </template>
      </SingleRequestBodyContent>
    </div>
    <SingleRequestBodyContent
      v-else
      :content="bodies.user_request_body || bodies.llm_request_body"
      title="Request Body"
    />

    <div
      v-if="
        bodies.user_response_body !== bodies.llm_response_body &&
        bodies.user_response_body &&
        bodies.llm_response_body
      "
      class="grid grid-cols-1 gap-4 md:grid-cols-2"
    >
      <SingleResponseBodyContent
        :content="bodies.llm_response_body"
        title="LLM Response Body"
        :status="status"
      />
      <SingleResponseBodyContent
        :content="bodies.user_response_body"
        title="User Response Body"
        :status="status"
      />
    </div>
    <SingleResponseBodyContent
      v-else
      :content="bodies.user_response_body || bodies.llm_response_body"
      title="Response Body"
      :status="status"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { Api } from '@/services/request';
import * as msgpack from '@msgpack/msgpack';
import { applyPatch } from 'fast-json-patch';
import { Button } from '@/components/ui/button';
import SingleRequestBodyContent from './SingleRequestBodyContent.vue';
import SingleResponseBodyContent from './SingleResponseBodyContent.vue';

const props = defineProps<{
  recordId: number;
  storageType: string;
  status: string | null;
}>();

const bodies = ref<any>(null);
const isLoadingBodies = ref(false);
const showPatched = ref(true);

const fetchAndDecodeBody = async () => {
  if (!props.storageType || !props.recordId) return;
  isLoadingBodies.value = true;
  try {
    const buffer = await Api.getRecordContent(props.recordId);
    const decoded = msgpack.decode(new Uint8Array(buffer)) as any;
    const textDecoder = new TextDecoder();
    bodies.value = {
      user_request_body: decoded.user_request_body ? textDecoder.decode(decoded.user_request_body) : null,
      llm_request_body: decoded.llm_request_body ? textDecoder.decode(decoded.llm_request_body) : null,
      user_response_body: decoded.user_response_body ? textDecoder.decode(decoded.user_response_body) : null,
      llm_response_body: decoded.llm_response_body ? textDecoder.decode(decoded.llm_response_body) : null,
    };
  } catch (error) {
    console.error("Failed to fetch or decode body content:", error);
  } finally {
    isLoadingBodies.value = false;
  }
};

const patchInfo = computed(() => {
  const userContent = bodies.value?.user_request_body;
  const llmContent = bodies.value?.llm_request_body;
  if (!userContent || !llmContent || userContent === llmContent) {
    return { isPatch: false, patchedContent: null };
  }
  try {
    const userJson = JSON.parse(userContent);
    const patch = JSON.parse(llmContent);
    if (Array.isArray(patch) && patch.every((op) => "op" in op && "path" in op)) {
      const { newDocument } = applyPatch(userJson, patch, true, false);
      return {
        isPatch: true,
        patchedContent: JSON.stringify(newDocument, null, 2),
      };
    }
  } catch (e) {}
  return { isPatch: false, patchedContent: null };
});

const llmContent = computed(() => {
  return patchInfo.value.isPatch && showPatched.value
    ? patchInfo.value.patchedContent
    : bodies.value?.llm_request_body;
});

const llmTitle = computed(() => {
  return patchInfo.value.isPatch && showPatched.value
    ? "LLM Request Body (Patched)"
    : "LLM Request Body (Raw Patch)";
});

onMounted(fetchAndDecodeBody);
</script>
