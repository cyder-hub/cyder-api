import { useI18n, type NamedValue } from "vue-i18n";
import type enMessages from "./locales/en/messages.json";

type AppMessages = typeof enMessages;
type DotJoin<Prefix extends string, Key extends string> = Prefix extends ""
  ? Key
  : `${Prefix}.${Key}`;

type LeafPaths<Node, Prefix extends string = ""> = {
  [Key in Extract<keyof Node, string>]: Node[Key] extends string
    ? DotJoin<Prefix, Key>
    : Node[Key] extends Record<string, unknown>
      ? LeafPaths<Node[Key], DotJoin<Prefix, Key>>
      : never;
}[Extract<keyof Node, string>];

type PathValue<Node, Path extends string> = Path extends `${infer Head}.${infer Tail}`
  ? Head extends keyof Node
    ? PathValue<Node[Head], Tail>
    : never
  : Path extends keyof Node
    ? Node[Path]
    : never;

type PlaceholderNames<Message extends string> =
  Message extends `${string}{${infer Param}}${infer Rest}`
    ? Param | PlaceholderNames<Rest>
    : never;

export type AppI18nKey = LeafPaths<AppMessages>;
export type AppI18nParamName<Key extends AppI18nKey> = PlaceholderNames<
  Extract<PathValue<AppMessages, Key>, string>
>;
export type AppI18nParams<Key extends AppI18nKey> = NamedValue<
  Record<AppI18nParamName<Key>, string | number | boolean | null | undefined>
>;

type AppI18nKeyWithParams = {
  [Key in AppI18nKey]: [AppI18nParamName<Key>] extends [never] ? never : Key;
}[AppI18nKey];
type AppI18nKeyWithoutParams = Exclude<AppI18nKey, AppI18nKeyWithParams>;

export interface AppTranslate {
  <Key extends AppI18nKeyWithoutParams>(key: Key): string;
  <Key extends AppI18nKey>(key: Key, params: AppI18nParams<Key>): string;
}

export function useAppI18n() {
  const composer = useI18n();

  const t = ((key: AppI18nKey, params?: NamedValue) => {
    return params === undefined ? composer.t(key) : composer.t(key, params);
  }) as AppTranslate;

  const te = (key: AppI18nKey) => composer.te(key);

  return {
    ...composer,
    t,
    te,
  };
}
