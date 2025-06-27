import { splitProps, type Component, type ComponentProps } from 'solid-js';
import { twMerge } from 'tailwind-merge';

// Table: <table> 的封装
const TableRoot: Component<ComponentProps<'table'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return (
    <table
      class={twMerge('w-full caption-bottom text-sm', local.class)}
      {...rest}
    />
  );
};

// TableHeader: <thead> 的封装
const TableHeader: Component<ComponentProps<'thead'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return <thead class={twMerge('bg-gray-100', local.class)} {...rest} />;
};

// TableBody: <tbody> 的封装
const TableBody: Component<ComponentProps<'tbody'>> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return <tbody class={twMerge("bg-white divide-y divide-gray-200", local.class)} {...rest} />
}

// TableRow: <tr> 的封装
const TableRow: Component<ComponentProps<'tr'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return (
    <tr
      class={twMerge(
        'transition-colors hover:bg-gray-50 data-[state=selected]:bg-gray-100',
        local.class
      )}
      {...rest}
    />
  );
};

// TableColumnHeader: <th> 的封装 (表头单元格)
const TableColumnHeader: Component<ComponentProps<'th'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return (
    <th
      class={twMerge(
        'px-4 py-3 text-left align-middle text-sm font-semibold text-gray-600 uppercase tracking-wider [&:has([role=checkbox])]:pr-0',
        local.class
      )}
      {...rest}
    />
  );
};

// TableCell: <td> 的封装
const TableCell: Component<ComponentProps<'td'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return (
    <td
      class={twMerge('px-4 py-2 align-middle text-sm text-gray-700 [&:has([role=checkbox])]:pr-0', local.class)}
      {...rest}
    />
  );
};

// TableCaption: <caption> 的封装
const TableCaption: Component<ComponentProps<'caption'>> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    return <caption class={twMerge("mt-4 text-sm text-gray-500", local.class)} {...rest} />
}

export {
  TableRoot,
  TableHeader,
  TableBody,
  TableRow,
  TableColumnHeader,
  TableCell,
  TableCaption,
};
