import { Component, For, createSignal } from 'solid-js';
import { Popover, PopoverTrigger, PopoverContent } from './ui/Popover';
import { Button } from './ui/Button';
import { setLocale, currentLocale } from '../i18n';

const GlobeIcon = () => (
    <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-gray-600 group-hover:text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9V3m0 18a9 9 0 009-9m-9 9a9 9 0 00-9-9" />
    </svg>
);

const LanguageSwitcher: Component = () => {
  const languages = [
    { code: 'en', name: 'English' },
    { code: 'zh', name: '中文' }
  ];
  const [isOpen, setIsOpen] = createSignal(false);

  const handleLanguageSelect = (langCode: string) => {
    setLocale(langCode);
    setIsOpen(false);
  };

  return (
    <div class="mt-auto p-4 border-t border-gray-200 flex justify-center">
      <Popover placement="top" open={isOpen()} onOpenChange={setIsOpen}>
        <PopoverTrigger
          as={(p) => (
            <Button
              {...p}
              variant="ghost"
              class="w-full flex justify-center items-center group"
              aria-label="Change language"
            >
              <GlobeIcon />
            </Button>
          )}
        />
        <PopoverContent class="p-1 w-40 mb-2">
          <div class="grid gap-1">
            <For each={languages}>
              {(lang) => (
                <Button
                  variant="ghost"
                  class="w-full justify-start"
                  classList={{ 'font-bold text-indigo-600 bg-indigo-100': currentLocale() === lang.code }}
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
