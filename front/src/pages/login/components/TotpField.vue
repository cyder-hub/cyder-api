<template>
  <div class="space-y-2">
    <div class="flex items-center justify-between gap-3">
      <label :for="inputId" class="block text-sm font-medium text-gray-700">
        {{ label }}
      </label>
      <span
        aria-hidden="true"
        class="font-mono text-[11px] tabular-nums text-gray-400"
      >
        {{ model.length }}/{{ CODE_LENGTH }}
      </span>
    </div>

    <div
      class="relative flex gap-1.5 sm:gap-2"
      :class="disabled ? 'cursor-not-allowed opacity-60' : 'cursor-text'"
    >
      <input
        :id="inputId"
        ref="inputRef"
        v-model="model"
        :disabled="disabled"
        :placeholder="placeholder"
        type="text"
        required
        inputmode="numeric"
        enterkeyhint="done"
        autocomplete="one-time-code"
        name="totp"
        autocapitalize="off"
        :spellcheck="false"
        pattern="[0-9]{6}"
        :maxlength="CODE_LENGTH"
        class="absolute inset-0 z-10 h-full w-full cursor-text border-0 bg-transparent text-transparent caret-transparent outline-none placeholder:text-transparent [-webkit-text-fill-color:transparent] disabled:cursor-not-allowed"
        @click="moveCaretToEnd"
        @focus="handleFocus"
        @blur="isFocused = false"
        @input="normalize"
        @change="normalize"
        @paste="handlePaste"
      />

      <span
        v-for="(digit, index) in digits"
        :key="index"
        aria-hidden="true"
        class="pointer-events-none flex h-12 min-w-0 flex-1 items-center justify-center rounded-lg border font-mono text-xl font-semibold tabular-nums shadow-xs transition-[border-color,background-color,box-shadow] duration-150"
        :class="[
          digit
            ? 'border-gray-300 bg-gray-50 text-gray-950'
            : 'border-gray-200 bg-white text-gray-400',
          activeIndex === index
            ? 'border-gray-900 bg-white ring-2 ring-gray-900/10'
            : '',
          index === 3 ? 'ml-1 sm:ml-1.5' : '',
        ]"
      >
        <span v-if="digit">{{ digit }}</span>
        <span
          v-else
          class="h-1.5 w-1.5 rounded-full bg-gray-200 transition-colors"
          :class="activeIndex === index ? 'bg-gray-400' : ''"
        />
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, useId } from "vue";

const CODE_LENGTH = 6;

defineProps<{
  label: string;
  placeholder: string;
  disabled: boolean;
}>();

const model = defineModel<string>({ required: true });
const inputId = useId();
const inputRef = ref<HTMLInputElement | null>(null);
const isFocused = ref(false);

const digits = computed(() =>
  Array.from(
    { length: CODE_LENGTH },
    (_, index) => model.value[index] ?? "",
  ),
);

const activeIndex = computed(() =>
  isFocused.value && model.value.length < CODE_LENGTH
    ? model.value.length
    : -1,
);

const normalizeCode = (value: string): string =>
  value.replace(/\D/g, "").slice(0, CODE_LENGTH);

const normalize = (event: Event): void => {
  const input = event.target as HTMLInputElement;
  const normalized = normalizeCode(input.value);
  input.value = normalized;
  model.value = normalized;
};

const handlePaste = (event: ClipboardEvent): void => {
  const pastedText = event.clipboardData?.getData("text");
  if (pastedText === undefined) return;

  const normalized = normalizeCode(pastedText);
  if (!normalized) return;

  event.preventDefault();
  model.value = normalized;
  if (inputRef.value) inputRef.value.value = normalized;
  void nextTick(moveCaretToEnd);
};

const moveCaretToEnd = (): void => {
  const end = model.value.length;
  inputRef.value?.setSelectionRange(end, end);
};

const handleFocus = async (): Promise<void> => {
  isFocused.value = true;
  await nextTick();
  moveCaretToEnd();
};
</script>
