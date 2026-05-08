const LANG_STORAGE_KEY = "lang";

const INTL_LOCALE_BY_APP_LOCALE: Record<string, string> = {
  en: "en-US",
  zh: "zh-CN",
};

const dateTimeFormatOptions: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
};

function getCurrentAppLocale(): string {
  if (typeof localStorage === "undefined") {
    return "en";
  }

  return localStorage.getItem(LANG_STORAGE_KEY) || "en";
}

function resolveIntlLocale(locale?: string | null): string {
  if (!locale) {
    return INTL_LOCALE_BY_APP_LOCALE[getCurrentAppLocale()] ?? "en-US";
  }

  return INTL_LOCALE_BY_APP_LOCALE[locale] ?? locale;
}

export function formatTimestamp(
  ms: number | undefined | null,
  locale?: string | null,
): string {
  if (!ms) return "";

  try {
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) return "";

    return new Intl.DateTimeFormat(
      resolveIntlLocale(locale),
      dateTimeFormatOptions,
    ).format(date);
  } catch {
    return "";
  }
}
