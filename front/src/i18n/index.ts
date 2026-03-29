import { createI18n } from "vue-i18n";
import { enDict } from "./en";
import { zhDict } from "./zh";

export const LANG_STORAGE_KEY = "lang";

const getInitialLocale = () => {
  const storedLang = localStorage.getItem(LANG_STORAGE_KEY);
  if (storedLang && (storedLang === "en" || storedLang === "zh")) {
    return storedLang;
  }
  return "en";
};

const i18n = createI18n({
  legacy: false, // Use Composition API
  locale: getInitialLocale(),
  fallbackLocale: "en",
  messages: {
    en: enDict,
    zh: zhDict,
  },
});

export default i18n;
