import { createRootRoute, Outlet } from '@tanstack/solid-router';
import { GlobalToaster } from '../components/GlobalMessage';

export const Route = createRootRoute({
    component: () => (
        <>
            <Outlet />
            <GlobalToaster regionId="global-message-region" />
        </>
    ),
});
