import { Component, For, createSignal, Show } from 'solid-js';
import { Popover, PopoverTrigger, PopoverContent } from './ui/Popover';
import { Button } from './ui/Button';
import { setLocale, currentLocale } from '../i18n';

const GlobeIcon = () => (
    <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9V3m0 18a9 9 0 009-9m-9 9a9 9 0 00-9-9" />
    </svg>
);

interface LanguageSwitcherProps {
  isCollapsed: boolean;
}

const LanguageSwitcher: Component<LanguageSwitcherProps> = (props) => {
  const languages = [
    { code: 'en', name: 'English' },
    { code: 'zh', name: '中文' }
  ];
  const [isOpen, setIsOpen] = createSignal(false);

  const handleLanguageSelect = (langCode: string) => {
    setLocale(langCode);
    setIsOpen(false);
  };

  const currentLanguage = () => languages.find(lang => lang.code === currentLocale());

  return (
    <div class="mt-auto px-4 py-2 border-t border-slate-700">
      <Popover placement="top" open={isOpen()} onOpenChange={setIsOpen}>
        <PopoverTrigger
          as={(p) => (
            <Button
              {...p}
              variant="ghost"
              class="w-full flex items-center py-2.5 px-4 rounded-md text-sm text-slate-300 hover:bg-slate-700 hover:text-white group"
              classList={{
                'justify-center': props.isCollapsed
              }}
              aria-label="Change language"
            >
              <span class="w-6 text-center flex-shrink-0">
                <GlobeIcon />
              </span>
              <Show when={!props.isCollapsed}>
                <span class="ml-3 whitespace-nowrap overflow-hidden">
                  {currentLanguage()?.name}
                </span>
              </Show>
            </Button>
          )}
        />
        <PopoverContent class="p-1 w-40 mb-2 bg-slate-900 border-slate-700 text-white">
          <div class="grid gap-1">
            <For each={languages}>
              {(lang) => (
                <Button
                  variant="ghost"
                  class="w-full justify-start text-white hover:bg-slate-700"
                  classList={{ 'font-bold bg-indigo-600 hover:bg-indigo-500': currentLocale() === lang.code }}
                  onClick={() => handleLanguageSelect(lang.code)}
                >
                  {lang.name}
                </Button>
              )}
            </For>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
};

export default LanguageSwitcher;
