import { createFileRoute, Outlet } from '@tanstack/solid-router';
import { Suspense } from 'solid-js';
import PageWrapper from '../components/PageWrapper';

export const Route = createFileRoute('/_layout')({
    component: () => (
        <PageWrapper>
            <Suspense>
                <Outlet />
            </Suspense>
        </PageWrapper>
    ),
});
