<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { Globe } from "lucide-vue-next";
import { LANG_STORAGE_KEY } from "@/i18n";

interface LanguageSwitcherProps {
  isCollapsed: boolean;
}

const props = defineProps<LanguageSwitcherProps>();

const { locale } = useI18n();
const languages = [
  { code: "en", name: "English" },
  { code: "zh", name: "中文" },
];
const isOpen = ref(false);

const handleLanguageSelect = (langCode: string) => {
  locale.value = langCode;
  localStorage.setItem(LANG_STORAGE_KEY, langCode);
  isOpen.value = false;
};

const currentLanguageName = () => {
  return languages.find((lang) => lang.code === locale.value)?.name;
};
</script>

<template>
  <div class="mt-auto p-2 border-t border-gray-100">
    <Popover v-model:open="isOpen">
      <PopoverTrigger as-child>
        <Button
          variant="ghost"
          class="w-full flex items-center py-1.5 px-2.5 rounded-md text-gray-500 hover:bg-gray-100 hover:text-gray-700 transition-colors h-auto border-0 text-sm"
          :class="{
            'justify-center': props.isCollapsed,
            'justify-start': !props.isCollapsed,
          }"
          aria-label="Change language"
        >
          <Globe class="h-4 w-4 flex-shrink-0" />
          <span
            v-if="!props.isCollapsed"
            class="ml-2 font-medium whitespace-nowrap overflow-hidden"
          >
            {{ currentLanguageName() }}
          </span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        class="p-1 w-36 mb-2 border-gray-200 bg-white text-gray-700 shadow-lg"
        :class="{ 'ml-2': props.isCollapsed }"
      >
        <div class="grid gap-0.5">
          <Button
            v-for="lang in languages"
            :key="lang.code"
            variant="ghost"
            class="w-full justify-start text-sm hover:bg-gray-100 hover:text-gray-900"
            :class="{
              'font-semibold text-blue-600 bg-blue-50': locale === lang.code,
            }"
            @click="handleLanguageSelect(lang.code)"
          >
            {{ lang.name }}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  </div>
</template>
