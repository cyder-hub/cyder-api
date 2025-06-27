import {
    Pagination as PaginationPrimitive,
    type PaginationRootProps,
    type PaginationItemProps,
    type PaginationEllipsisProps,
    type PaginationPreviousProps,
    type PaginationNextProps,
} from '@kobalte/core/pagination';
import { splitProps, type Component } from 'solid-js';
import { twMerge } from 'tailwind-merge';

const PaginationRoot: Component<PaginationRootProps> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return (
        <PaginationPrimitive
            class={twMerge('flex items-center gap-2 text-sm', local.class)}
            {...rest}
        />
    );
};

const PaginationItem: Component<PaginationItemProps> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return (
        <PaginationPrimitive.Item
            class={twMerge(
                'px-3 py-1.5 rounded border font-medium transition-colors duration-150 ease-in-out',
                'ui-current:bg-blue-600 ui-current:text-white ui-current:border-blue-600 ui-current:z-10',
                'bg-white text-gray-700 border-gray-300 hover:bg-gray-50',
                'ui-disabled:opacity-50 ui-disabled:cursor-not-allowed',
                local.class
            )}
            {...rest}
        />
    );
};

const PaginationEllipsis: Component<PaginationEllipsisProps> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return (
        <PaginationPrimitive.Ellipsis
            class={twMerge('px-3 py-1.5 text-gray-500', local.class)}
            {...rest}
        >
            ...
        </PaginationPrimitive.Ellipsis>
    );
};

const PaginationPrevious: Component<PaginationPreviousProps> = (props) => {
    const [local, rest] = splitProps(props, ['class', 'children']);
    return (
        <PaginationPrimitive.Previous
            class={twMerge(
                'px-3 py-1.5 rounded border border-gray-300 bg-white text-gray-600 hover:bg-gray-100 ui-disabled:opacity-50 ui-disabled:cursor-not-allowed transition duration-150 ease-in-out',
                local.class
            )}
            {...rest}
        >
            {local.children ?? '<'}
        </PaginationPrimitive.Previous>
    );
};

const PaginationNext: Component<PaginationNextProps> = (props) => {
    const [local, rest] = splitProps(props, ['class', 'children']);
    return (
        <PaginationPrimitive.Next
            class={twMerge(
                'px-3 py-1.5 rounded border border-gray-300 bg-white text-gray-600 hover:bg-gray-100 ui-disabled:opacity-50 ui-disabled:cursor-not-allowed transition duration-150 ease-in-out',
                local.class
            )}
            {...rest}
        >
            {local.children ?? '>'}
        </PaginationPrimitive.Next>
    );
};

const PaginationItems = PaginationPrimitive.Items;

export const Pagination = Object.assign(PaginationRoot, {
    Item: PaginationItem,
    Ellipsis: PaginationEllipsis,
    Previous: PaginationPrevious,
    Next: PaginationNext,
    Items: PaginationItems,
});
