import { type ParentComponent, createSignal, For, onMount, Show } from 'solid-js';
import { Link, useNavigate, useMatchRoute } from '@tanstack/solid-router';
import { getNavItems } from '../router'; // Import the helper to get navigation items
import { tryRefreshToken } from '../services/auth'; // Import the auth check function
import { useI18n } from '../i18n'; // Import useI18n
import LanguageSwitcher from './LanguageSwitcher'; // Import the new component

const navItems = getNavItems(); // Get the navigation items

const MenuIcon = () => (
    <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M4 6h16M4 12h16m-7 6h7" />
    </svg>
);

const XIcon = () => (
    <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
    </svg>
);

const PageWrapper: ParentComponent = (props) => {
  const [t] = useI18n(); // Initialize i18n
  const [isCollapsed, setIsCollapsed] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(true); // Start in loading state
  const [isAuthenticated, setIsAuthenticated] = createSignal(false); // Assume not authenticated initially
  const navigate = useNavigate();
  const matchRoute = useMatchRoute();

  onMount(async () => {
    const authenticated = await tryRefreshToken();
    if (authenticated) {
      setIsAuthenticated(true);
    } else {
      // Redirect to login page if not authenticated
      // Use the base path from App.tsx if necessary, assuming '/login' is relative to that base
      navigate({ to: '/login', replace: true });
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
            class="bg-slate-800 text-slate-300 shadow-md transition-all duration-300 ease-in-out flex flex-col flex-shrink-0 border-r border-slate-700"
            classList={{
              'w-64': !isCollapsed(), // Expanded width
              'w-20': isCollapsed()   // Collapsed width
            }}
          >
            <div
              class="flex items-center h-16 px-4 border-b border-slate-700 flex-shrink-0"
              classList={{
                'justify-between': !isCollapsed(),
                'justify-center': isCollapsed()
              }}
            >
              <Show when={!isCollapsed()}>
                <h1 class="text-lg font-bold text-white whitespace-nowrap">
                  {t('appHeader')}
                </h1>
              </Show>
              <button
                onClick={toggleSidebar}
                class="p-2 rounded-md text-slate-300 hover:bg-slate-700 hover:text-white focus:outline-none focus:ring-2 focus:ring-inset focus:ring-indigo-500"
                aria-label={t('toggleSidebar')}
              >
                <Show when={isCollapsed()} fallback={<XIcon />}>
                  <MenuIcon />
                </Show>
              </button>
            </div>
            <nav class="flex-grow overflow-y-auto overflow-x-hidden py-4">
              <ul class="space-y-2 px-4 list-none">
                <For each={navItems}>
                  {(item) => (
                    <li>
                      <Link
                        to={item.path}
                        class="w-full flex items-center py-2.5 px-4 rounded-md text-sm text-slate-300 hover:bg-slate-700 hover:text-white group"
                        classList={{
                          'bg-indigo-600 text-white font-semibold hover:bg-indigo-500': !!matchRoute({ to: item.path, fuzzy: item.path === '/dashboard' }),
                          'justify-center': isCollapsed()
                        }}
                      >
                        <span class="w-6 text-center flex-shrink-0">
                          {item.icon}
                        </span>
                        <Show when={!isCollapsed()}>
                          <span class="ml-3 whitespace-nowrap overflow-hidden">
                            {translateNavItem(item)}
                          </span>
                        </Show>
                      </Link>
                    </li>
                  )}
                </For>
              </ul>
            </nav>
            <LanguageSwitcher isCollapsed={isCollapsed()} />
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
