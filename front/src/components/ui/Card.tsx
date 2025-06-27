import { splitProps, type Component, type ComponentProps } from 'solid-js';
import { twMerge } from 'tailwind-merge';

const Card: Component<ComponentProps<'div'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return (
        <div
            class={twMerge('bg-white rounded-lg shadow-md border border-gray-200', local.class)}
            {...rest}
        />
    );
};

const CardHeader: Component<ComponentProps<'div'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return <div class={twMerge('flex flex-col space-y-1.5 p-6', local.class)} {...rest} />;
};

const CardTitle: Component<ComponentProps<'h2'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return (
        <h2
            class={twMerge('text-xl font-semibold leading-none tracking-tight text-gray-700', local.class)}
            {...rest}
        />
    );
};

const CardDescription: Component<ComponentProps<'p'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return <p class={twMerge('text-sm text-gray-500', local.class)} {...rest} />;
};

const CardContent: Component<ComponentProps<'div'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return <div class={twMerge('p-6 pt-0', local.class)} {...rest} />;
};

const CardFooter: Component<ComponentProps<'div'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return <div class={twMerge('flex items-center p-6 pt-0', local.class)} {...rest} />;
};

export { Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter };
