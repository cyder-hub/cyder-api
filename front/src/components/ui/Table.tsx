import { splitProps, type Component, type ComponentProps, createContext, useContext } from 'solid-js';
import { twMerge } from 'tailwind-merge';
import { cva, type VariantProps } from 'class-variance-authority';

type TableSize = 'default' | 'small' | 'large';
interface TableContextType {
    size?: TableSize | null;
}
const TableContext = createContext<TableContextType>();

export interface TableRootProps extends ComponentProps<'table'> {
    size?: TableSize;
}

// Wrapper for <table>
const TableRoot: Component<TableRootProps> = (props) => {
  const [local, rest] = splitProps(props, ['class', 'size']);
  return (
    <TableContext.Provider value={{ size: local.size }}>
        <table
          class={twMerge('w-full caption-bottom text-sm', local.class)}
          {...rest}
        />
    </TableContext.Provider>
  );
};

// Wrapper for <thead>
const TableHeader: Component<ComponentProps<'thead'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return <thead class={twMerge('border-b border-gray-200', local.class)} {...rest} />;
};

// Wrapper for <tbody>
const TableBody: Component<ComponentProps<'tbody'>> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return <tbody class={twMerge("bg-white divide-y divide-gray-200", local.class)} {...rest} />
}

// Wrapper for <tr>
const TableRow: Component<ComponentProps<'tr'>> = (props) => {
  const [local, rest] = splitProps(props, ['class']);
  return (
    <tr
      class={twMerge(
        'transition-colors hover:bg-gray-50 data-[state=selected]:bg-slate-100',
        local.class
      )}
      {...rest}
    />
  );
};

const tableColumnHeaderVariants = cva(
    'text-left align-middle font-semibold text-gray-500 uppercase tracking-wider [&:has([role=checkbox])]:pr-0',
    {
        variants: {
            size: {
                default: 'px-4 py-3 text-sm',
                small: 'px-2 py-2 text-xs',
                large: 'px-6 py-4 text-base',
            },
        },
        defaultVariants: {
            size: 'default',
        },
    }
);

export interface TableColumnHeaderProps extends ComponentProps<'th'> {}

// Wrapper for <th> (header cell)
const TableColumnHeader: Component<TableColumnHeaderProps> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    const context = useContext(TableContext);
    return (
        <th
            class={twMerge(tableColumnHeaderVariants({ size: context?.size }), local.class)}
            {...rest}
        />
    );
};

const tableCellVariants = cva(
    'align-middle text-gray-700 [&:has([role=checkbox])]:pr-0',
    {
        variants: {
            size: {
                default: 'px-4 py-2 text-sm',
                small: 'px-2 py-1 text-xs',
                large: 'px-6 py-4 text-base',
            },
        },
        defaultVariants: {
            size: 'default',
        },
    }
);

export interface TableCellProps extends ComponentProps<'td'> {}

// Wrapper for <td>
const TableCell: Component<TableCellProps> = (props) => {
    const [local, rest] = splitProps(props, ['class']);
    const context = useContext(TableContext);
    return (
        <td
            class={twMerge(tableCellVariants({ size: context?.size }), local.class)}
            {...rest}
        />
    );
};

// Wrapper for <caption>
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
