import {
    TextField as TextFieldPrimitive,
    type TextFieldRootProps,
} from '@kobalte/core/text-field';
import {
    NumberField as NumberFieldPrimitive,
    type NumberFieldRootProps,
} from '@kobalte/core/number-field';
import { splitProps, type Component } from 'solid-js';
import { twMerge } from 'tailwind-merge';

// TextField
interface TextFieldProps extends Omit<TextFieldRootProps, 'children'> {
    label?: string;
    placeholder?: string;
    type?: 'text' | 'password' | 'email' | 'search' | 'tel' | 'url' | 'datetime-local';
    class?: string;
    textarea?: boolean;
    rows?: number;
}

const TextField: Component<TextFieldProps> = (props) => {
    const [local, rest] = splitProps(props, ['label', 'placeholder', 'type', 'class', 'textarea', 'rows']);

    return (
        <TextFieldPrimitive {...rest} class={twMerge('flex flex-col space-y-1.5', local.class)}>
            {local.label && <TextFieldPrimitive.Label class="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70">{local.label}</TextFieldPrimitive.Label>}
            <TextFieldPrimitive.Input
                as={local.textarea ? 'textarea' : 'input'}
                rows={local.rows}
                type={local.type}
                class={twMerge(
                    "flex w-full rounded-md border border-gray-300 bg-transparent px-3 py-2 text-sm ring-offset-white placeholder:text-gray-400 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50",
                    local.textarea ? 'min-h-[80px]' : 'h-10'
                )}
                placeholder={local.placeholder}
            />
        </TextFieldPrimitive>
    );
};

// NumberField
interface NumberFieldProps extends Omit<NumberFieldRootProps, 'children'> {
    label?: string;
    placeholder?: string;
    class?: string;
}

const NumberField: Component<NumberFieldProps> = (props) => {
    const [local, rest] = splitProps(props, ['label', 'placeholder', 'class']);

    return (
        <NumberFieldPrimitive {...rest} class={twMerge('flex flex-col space-y-1.5', local.class)}>
            {local.label && <NumberFieldPrimitive.Label class="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70">{local.label}</NumberFieldPrimitive.Label>}
            <NumberFieldPrimitive.Input
                class="flex h-10 w-full rounded-md border border-gray-300 bg-transparent px-3 py-2 text-sm ring-offset-white placeholder:text-gray-400 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                placeholder={local.placeholder}
            />
        </NumberFieldPrimitive>
    );
};

export { TextField, NumberField };
