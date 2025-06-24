import type { Component } from 'solid-js';
import { Router, Route } from '@solidjs/router';
import { lazy, For } from 'solid-js';
import { loginRoute, mainRoutes } from './router'; // Import the separated login route and main routes
// I18nContextProvider is no longer needed as @solid-primitives/i18n manages global state
import { GlobalToaster } from './components/GlobalMessage'; // Import the GlobalToaster

// Lazy load the PageWrapper component
const PageWrapper = lazy(() => import('./components/PageWrapper'));

// loginRoute and mainRoutes are now directly imported

const App: Component = () => {
  return (
    // <I18nContextProvider> is removed.
    // The i18n instance created in i18n.ts is globally available.
    <>
      <Router base="/ai/manager/ui">
        {/* Render the login route directly */}
        <Route path={loginRoute.path} component={loginRoute.component} />

        {/* Base route using PageWrapper as layout for main routes */}
        <Route component={PageWrapper}>
          <For each={mainRoutes}>
            {(route) => <Route path={route.path} component={route.component} />}
          </For>
        </Route>
      </Router>
      <GlobalToaster regionId="global-message-region" />
    </>
  );
};

export default App;
