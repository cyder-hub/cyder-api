import {
    Popover as PopoverPrimitive,
    type PopoverRootProps,
    type PopoverContentProps,
} from '@kobalte/core/popover';
import { splitProps, type Component } from 'solid-js';
import { twMerge } from 'tailwind-merge';

const Popover: Component<PopoverRootProps> = (props) => {
    return <PopoverPrimitive {...props} />;
};

const PopoverTrigger = PopoverPrimitive.Trigger;

const PopoverContent: Component<PopoverContentProps> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return (
        <PopoverPrimitive.Portal>
            <PopoverPrimitive.Content
                class={twMerge(
                    'bg-white border border-gray-300 rounded-md shadow-lg z-50',
                    'data-[expanded]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[expanded]:fade-in-0 data-[closed]:zoom-out-95 data-[expanded]:zoom-in-95',
                    local.class
                )}
                {...rest}
            />
        </PopoverPrimitive.Portal>
    );
};

export { Popover, PopoverTrigger, PopoverContent };
