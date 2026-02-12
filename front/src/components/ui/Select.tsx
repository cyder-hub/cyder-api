import {
  Select as SelectPrimitive,
  type SelectContentProps,
  type SelectItemProps,
  type SelectRootProps,
  type SelectTriggerProps,
  type ItemComponentProps,
} from '@kobalte/core/select';
import { splitProps, type Component } from 'solid-js';
import { twMerge } from 'tailwind-merge';

const Root: Component<SelectRootProps> = (props) => {
    return <SelectPrimitive {...props} />;
};

// Styled Trigger
const SelectTrigger: Component<SelectTriggerProps> = (props) => {
  const [local, rest] = splitProps(props, ['class', 'children']);
  return (
    <SelectPrimitive.Trigger
      class={twMerge(
        'group flex h-10 w-full items-center justify-between rounded-md border border-gray-300 bg-transparent px-3 py-2 text-sm ring-offset-white placeholder:text-gray-400 transition-colors focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:cursor-not-allowed disabled:opacity-50',
        local.class
      )}
      {...rest}
    >
      {local.children}
      <SelectPrimitive.Icon>
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          class="h-4 w-4 opacity-50 transition-transform group-data-[expanded]:rotate-180"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </SelectPrimitive.Icon>
    </SelectPrimitive.Trigger>
  );
};

// Styled Content
const SelectContent: Component<SelectContentProps> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return (
    <SelectPrimitive.Portal>
      <SelectPrimitive.Content
        class={twMerge(
          'relative z-50 min-w-[8rem] overflow-hidden rounded-md bg-white text-gray-800 shadow-2xl data-[expanded]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[expanded]:fade-in-0 data-[closed]:zoom-out-95 data-[expanded]:zoom-in-95 data-[popper-placement=bottom]:mt-2 data-[popper-placement=top]:-mt-2',
          'max-h-96 overflow-y-auto',
          local.class
        )}
        {...rest}
      >
        <SelectPrimitive.Listbox class="p-1" />
      </SelectPrimitive.Content>
    </SelectPrimitive.Portal>
  );
};

// Styled Item
const SelectItem: Component<SelectItemProps> = (props) => {
    const [local, rest] = splitProps(props, ["class", "children"]);
    return (
        <SelectPrimitive.Item
            class={twMerge(
                "relative flex w-full cursor-default select-none items-center rounded-sm py-1.5 pl-2 pr-2 text-sm outline-none focus:bg-indigo-100 focus:text-indigo-900 data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
                local.class
            )}
            {...rest}
        >
            <span class="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
                <SelectPrimitive.ItemIndicator>
                    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-4 w-4">
                        <path d="M20 6 9 17l-5-5" />
                    </svg>
                </SelectPrimitive.ItemIndicator>
            </span>
            <div class="pl-6 w-full">{local.children}</div>
        </SelectPrimitive.Item>
    );
};

const SelectParts = Object.assign(Root, {
    Label: SelectPrimitive.Label,
    Trigger: SelectTrigger,
    Value: SelectPrimitive.Value,
    Content: SelectContent,
    Item: SelectItem,
    ItemLabel: SelectPrimitive.ItemLabel,
    ItemDescription: SelectPrimitive.ItemDescription,
    Group: SelectPrimitive.Group,
    GroupLabel: SelectPrimitive.GroupLabel,
});

import { JSX } from 'solid-js';

interface SelectProps<T = any> extends Omit<SelectRootProps<T>, 'children' | 'itemComponent'> {
    label?: string;
    placeholder?: string;
    itemComponent?: (props: ItemComponentProps<T>) => JSX.Element;
    class?: string;
}

const Select = <T>(props: SelectProps<T>) => {
    const [local, rest] = splitProps(props, ['label', 'placeholder', 'itemComponent', 'class', 'optionTextValue', 'optionValue']);

    const defaultItemComponent = (itemProps: ItemComponentProps<T>) => (
        <SelectParts.Item item={itemProps.item}>
            <SelectParts.ItemLabel>{(itemProps.item.rawValue as any)?.[local.optionTextValue] ?? itemProps.item.rawValue}</SelectParts.ItemLabel>
        </SelectParts.Item>
    );

    return (
        <SelectParts<T>
            {...rest}
            itemComponent={local.itemComponent || defaultItemComponent}
            optionValue={local.optionValue}
            optionTextValue={local.optionTextValue}
            class={twMerge('flex flex-col space-y-1.5', local.class)}
        >
            {local.label && <SelectParts.Label class="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70">{local.label}</SelectParts.Label>}
            <SelectParts.Trigger>
                <SelectParts.Value<T> placeholder={local.placeholder}>
                    {state => local.optionTextValue ? (state.selectedOption() as any)?.[local.optionTextValue] : state.selectedOption()}
                </SelectParts.Value>
            </SelectParts.Trigger>
            <SelectParts.Content />
        </SelectParts>
    );
};

export { Select };
