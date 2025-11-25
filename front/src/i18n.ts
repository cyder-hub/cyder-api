import { createSignal, Accessor, Setter, createMemo } from 'solid-js';
import { flatten, translator, resolveTemplate } from '@solid-primitives/i18n';
import { enDict } from './i18n/en';
import { zhDict } from './i18n/zh';

const dictionaries = {
    en: enDict,
    zh: zhDict,
};

// Default language
const defaultLang = "en";

// Type for the translation function arguments, matching resolveTemplate
type TranslationArgs = Record<string, string | number>;

// Type for the translation function itself
type TranslatorFn = (key: string, params?: TranslationArgs, defaultValue?: string) => string;

// Type for the value returned by useI18n
export type I18nInterface = [
    TranslatorFn,
    {
        locale: Accessor<string>;
        setLocale: Setter<string>;
    }
];

// LocalStorage key for language
const LANG_STORAGE_KEY = 'lang';

// Define a type for the keys of the dictionaries object
type LangKey = keyof typeof dictionaries;

// Function to get initial locale
const getInitialLocale = (): LangKey => {
    const storedLang = localStorage.getItem(LANG_STORAGE_KEY);
    if (storedLang && storedLang in dictionaries) {
        return storedLang as LangKey;
    }
    // You might want to add browser language detection here as a fallback
    return defaultLang as LangKey;
};

// Create a signal for the current locale
const [locale, _setLocaleSignal] = createSignal<LangKey>(getInitialLocale());

// Wrapper for setLocale to also save to localStorage
const setLocaleSignal: Setter<string> = (langOrFn: string | ((prev: string) => string)) => {
    const newLang = typeof langOrFn === 'function' ? langOrFn(locale()) : langOrFn;
    if (newLang in dictionaries) {
        _setLocaleSignal(newLang as LangKey);
        localStorage.setItem(LANG_STORAGE_KEY, newLang);
    } else {
        console.warn(`Language "${newLang}" not supported. Falling back to default or current.`);
        // Optionally, fall back to default or do nothing
        // localStorage.setItem(LANG_STORAGE_KEY, defaultLang);
        // _setLocaleSignal(defaultLang as LangKey);
    }
};

const dict = createMemo(() => flatten(dictionaries[locale() as LangKey]));

const tFunction = translator(dict, resolveTemplate) as TranslatorFn; // Cast to the simplified TranslatorFn

// The value that useI18n will provide
const i18nInstanceValue: I18nInterface = [
    tFunction, // Now tFunction's asserted type matches TranslatorFn in I18nInterface
    {
        locale: locale, // Accessor for the current locale
        setLocale: setLocaleSignal // Setter to change the locale
    }
];

// Create the useI18n hook
export const useI18n = (): I18nInterface => {
  return i18nInstanceValue;
};

// Optionally, you can export the t function and locale actions directly from the instance
export const t = tFunction;
export const actions = {
    locale: locale,
    setLocale: setLocaleSignal,
};
export const setLocale = setLocaleSignal;
export const currentLocale = locale;
