export const APP_LOCALES = ["en", "zh"] as const;
export type AppLocale = (typeof APP_LOCALES)[number];

export const DEFAULT_APP_LOCALE: AppLocale = "en";
export const LANG_STORAGE_KEY = "lang";

const INTL_LOCALE_BY_APP_LOCALE: Record<AppLocale, string> = {
  en: "en-US",
  zh: "zh-CN",
};

function getAppLocaleStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export function isAppLocale(value: string | null | undefined): value is AppLocale {
  return APP_LOCALES.some((locale) => locale === value);
}

export function resolveAppLocale(value: string | null | undefined): AppLocale {
  return isAppLocale(value) ? value : DEFAULT_APP_LOCALE;
}

export function getStoredAppLocale(): AppLocale {
  return resolveAppLocale(getAppLocaleStorage()?.getItem(LANG_STORAGE_KEY));
}

export function setStoredAppLocale(locale: AppLocale): void {
  getAppLocaleStorage()?.setItem(LANG_STORAGE_KEY, locale);
}

export function resolveIntlLocale(locale?: string | null): string {
  if (locale) {
    return INTL_LOCALE_BY_APP_LOCALE[locale as AppLocale] ?? locale;
  }

  return INTL_LOCALE_BY_APP_LOCALE[getStoredAppLocale()];
}
