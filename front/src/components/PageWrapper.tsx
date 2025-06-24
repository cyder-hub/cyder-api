import { type ParentComponent, createSignal, For, onMount, Show } from 'solid-js';
import { A, useNavigate } from '@solidjs/router';
import { getNavItems } from '../router'; // Import the helper to get navigation items
import { tryRefreshToken } from '../services/auth'; // Import the auth check function
import { useI18n, setLocale, currentLocale } from '../i18n'; // Import useI18n and locale functions

const navItems = getNavItems(); // Get the navigation items

const PageWrapper: ParentComponent = (props) => {
  const [t] = useI18n(); // Initialize i18n
  const [isCollapsed, setIsCollapsed] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(true); // Start in loading state
  const [isAuthenticated, setIsAuthenticated] = createSignal(false); // Assume not authenticated initially
  const navigate = useNavigate();

  onMount(async () => {
    const authenticated = await tryRefreshToken();
    if (authenticated) {
      setIsAuthenticated(true);
    } else {
      // Redirect to login page if not authenticated
      // Use the base path from App.tsx if necessary, assuming '/login' is relative to that base
      navigate('/login', { replace: true });
    }
    setIsLoading(false); // Finish loading check
  });

  const toggleSidebar = () => {
    setIsCollapsed(!isCollapsed());
  };

  // Helper to translate nav item text using i18nKey from RouteConfig
  const translateNavItem = (item: { text?: string, i18nKey?: string }) => {
    if (item.i18nKey) {
      return t(item.i18nKey);
    }
    return item.text || ''; // Fallback to original text if no i18nKey
  };

  return (
    <Show when={!isLoading()} fallback={<div>{t('loading')}</div>}> {/* Show loading indicator */}
      <Show when={isAuthenticated()}> {/* Only render layout if authenticated */}
        <div class="flex min-h-screen bg-gray-100"> {/* Replaced pageContainer */}
          {/* Sidebar */}
          <aside
            class="bg-white shadow-md transition-all duration-300 ease-in-out relative flex-shrink-0 overflow-hidden border-r border-gray-200"
            classList={{
              'w-60': !isCollapsed(), // Expanded width (adjust as needed)
              'w-20': isCollapsed()   // Collapsed width (adjust as needed)
            }}
          >
            <div class="p-4 flex justify-end"> {/* Container for the button */}
              <button
                onClick={toggleSidebar}
                class="p-2 rounded-md hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-inset focus:ring-indigo-500" // Replaced toggleButton
                aria-label={t('toggleSidebar')}
              >
                {isCollapsed() ? '☰' : '✕'} {/* Simple icons, replace with actual icons later */}
              </button>
            </div>
            <nav class="mt-4"> {/* Replaced navList margin-top */}
              <ul class="space-y-2 px-4 list-none"> {/* Added list-none */}
                <For each={navItems}>
                  {(item) => (
                    <li>
                      <A
                        href={item.path} // Use path instead of href
                        class="flex items-center py-2.5 px-4 rounded-md text-gray-700 hover:bg-indigo-50 hover:text-indigo-700 group"
                        activeClass="bg-indigo-100 text-indigo-800 font-semibold"
                      >
                        <span class="w-6 mr-3 text-center flex-shrink-0">{item.icon}</span>
                        <span
                          class="transition-opacity duration-200 ease-in-out whitespace-nowrap overflow-hidden"
                          classList={{
                            'opacity-0 w-0': isCollapsed(),
                            'opacity-100': !isCollapsed()
                          }}
                        >
                          {translateNavItem(item)}
                        </span>
                      </A>
                    </li>
                  )}
                </For>
              </ul>
            </nav>
          </aside>

          {/* Main content area */}
          <div class="flex-grow flex flex-col"> {/* Replaced mainContentWrapper */}
            <header class="bg-white shadow-sm p-4 flex justify-between items-center"> {/* Replaced header */}
              <div>{t('appHeader')}</div>
              <div class="flex items-center space-x-2">
                <button
                  onClick={() => setLocale('en')}
                  class="px-3 py-1 text-sm rounded-md hover:bg-gray-200"
                  classList={{ 'font-bold text-indigo-600 bg-indigo-100': currentLocale() === 'en' }}
                >
                  {t('language.english')}
                </button>
                <span class="text-gray-400">|</span>
                <button
                  onClick={() => setLocale('zh')}
                  class="px-3 py-1 text-sm rounded-md hover:bg-gray-200"
                  classList={{ 'font-bold text-indigo-600 bg-indigo-100': currentLocale() === 'zh' }}
                >
                  {t('language.chinese')}
                </button>
              </div>
            </header>
            <main class="flex-grow p-6 overflow-y-auto"> {/* Replaced mainContent */}
              {/* props.children renders the matched child route component */}
              { props.children }
            </main>
            <footer class="bg-white border-t border-gray-200 p-4 text-center text-sm text-gray-500 mt-auto"> {/* Replaced footer */}
              {t('appFooter')}
            </footer>
          </div>
        </div>
      </Show>
    </Show>
  );
}
export default PageWrapper;
