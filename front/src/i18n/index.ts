import { createI18n } from "vue-i18n";
import enMessages from "./locales/en/messages.json";
import zhMessages from "./locales/zh/messages.json";
import { DEFAULT_APP_LOCALE, getStoredAppLocale } from "./locale";

const getInitialLocale = () => {
  return getStoredAppLocale();
};

const i18n = createI18n({
  legacy: false, // Use Composition API
  locale: getInitialLocale(),
  fallbackLocale: DEFAULT_APP_LOCALE,
  messages: {
    en: enMessages,
    zh: zhMessages,
  },
});

export {
  APP_LOCALES,
  DEFAULT_APP_LOCALE,
  LANG_STORAGE_KEY,
  getStoredAppLocale,
  isAppLocale,
  resolveIntlLocale,
  resolveAppLocale,
  setStoredAppLocale,
} from "./locale";
export {
  useAppI18n,
} from "./typed";
export type {
  AppI18nKey,
  AppI18nParamName,
  AppI18nParams,
  AppTranslate,
} from "./typed";
export type { AppLocale } from "./locale";

export default i18n;
