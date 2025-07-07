import { type ParentComponent, createSignal, For, onMount, Show } from 'solid-js';
import { A, useNavigate } from '@solidjs/router';
import { getNavItems } from '../router'; // Import the helper to get navigation items
import { tryRefreshToken } from '../services/auth'; // Import the auth check function
import { useI18n } from '../i18n'; // Import useI18n
import LanguageSwitcher from './LanguageSwitcher'; // Import the new component

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
        <div class="flex h-screen bg-gray-100"> {/* Replaced pageContainer */}
          {/* Sidebar */}
          <aside
            class="bg-white shadow-md transition-all duration-300 ease-in-out flex flex-col flex-shrink-0 border-r border-gray-200"
            classList={{
              'w-64': !isCollapsed(), // Expanded width
              'w-20': isCollapsed()   // Collapsed width
            }}
          >
            <div
              class="flex items-center h-16 px-4 border-b border-gray-200 flex-shrink-0"
              classList={{
                'justify-between': !isCollapsed(),
                'justify-center': isCollapsed()
              }}
            >
              <Show when={!isCollapsed()}>
                <h1 class="text-lg font-bold text-indigo-600 whitespace-nowrap">
                  {t('appHeader')}
                </h1>
              </Show>
              <button
                onClick={toggleSidebar}
                class="p-2 rounded-md hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-inset focus:ring-indigo-500"
                aria-label={t('toggleSidebar')}
              >
                {isCollapsed() ? '☰' : '✕'}
              </button>
            </div>
            <nav class="flex-grow overflow-y-auto overflow-x-hidden py-4">
              <ul class="space-y-2 px-4 list-none">
                <For each={navItems}>
                  {(item) => (
                    <li>
                      <A
                        href={item.path}
                        class="flex items-center py-2.5 px-4 rounded-md text-gray-700 hover:bg-indigo-50 hover:text-indigo-700 group"
                        activeClass="bg-indigo-100 text-indigo-800 font-semibold"
                        classList={{ 'justify-center': isCollapsed() }}
                      >
                        <span class="w-6 text-center flex-shrink-0" classList={{ 'mr-3': !isCollapsed() }}>
                          {item.icon}
                        </span>
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
            <LanguageSwitcher />
          </aside>

          {/* Main content area */}
          <div class="flex-grow flex flex-col"> {/* Replaced mainContentWrapper */}
            <main class="flex-grow p-6 overflow-y-auto"> {/* Replaced mainContent */}
              {/* props.children renders the matched child route component */}
              { props.children }
            </main>
          </div>
        </div>
      </Show>
    </Show>
  );
}
export default PageWrapper;
