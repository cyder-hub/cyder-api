import { createFileRoute, redirect } from '@tanstack/solid-router';

export const Route = createFileRoute('/_layout/$')({
    beforeLoad: () => {
        throw redirect({
            to: '/dashboard',
            replace: true,
        });
    },
});
