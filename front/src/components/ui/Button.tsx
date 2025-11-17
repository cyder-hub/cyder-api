import { splitProps, type Component, type ComponentProps } from 'solid-js';
import { Button as ButtonPrimitive } from '@kobalte/core/button';
import { cva, type VariantProps } from 'class-variance-authority';
import { twMerge } from 'tailwind-merge';

// 1. 使用 cva 定义按钮的样式变体
const buttonVariants = cva(
  // Base classes: 应用于所有变体的基础样式
  'inline-flex items-center justify-center rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50',
  {
    variants: {
      // Variant type: "variant"
      variant: {
        primary: 'bg-blue-600 text-white hover:bg-blue-700',
        secondary: 'bg-gray-200 text-gray-800 hover:bg-gray-300',
        destructive: 'bg-red-600 text-white hover:bg-red-700',
        ghost: 'hover:bg-gray-100',
      },
      // Variant type: "type"
      type: {
        button: '',
        text: 'bg-transparent hover:bg-transparent underline-offset-4 hover:underline',
      },
      // Variant type: "size"
      size: {
        sm: 'h-9 px-3',
        md: 'h-10 px-4 py-2',
        lg: 'h-11 px-8',
      },
    },
    compoundVariants: [
      {
        type: 'text',
        class: 'h-auto p-0',
      },
      {
        type: 'text',
        variant: 'primary',
        class: 'text-blue-600 hover:text-blue-700',
      },
      {
        type: 'text',
        variant: 'destructive',
        class: 'text-red-600 hover:text-red-700',
      },
      {
        type: 'text',
        variant: 'secondary',
        class: 'text-gray-700 hover:text-gray-900',
      },
      {
        type: 'text',
        variant: 'ghost',
        class: 'text-blue-600 hover:text-blue-700',
      },
    ],
    // Default variants
    defaultVariants: {
      variant: 'primary',
      size: 'md',
      type: 'button',
    },
  }
);

// 2. 定义组件的 Props 类型
// - 继承 Kobalte Button 的所有 Props
// - 添加 cva 定义的变体 Props
export interface ButtonProps
  extends ComponentProps<typeof ButtonPrimitive>,
    VariantProps<typeof buttonVariants> {}

// 3. 创建封装后的 Button 组件
export const Button: Component<ButtonProps> = (props) => {
  // 使用 splitProps 将变体 props 和其他 props 分开
  // 这是 SolidJS 的最佳实践，确保响应性
  // 使用 splitProps 将变体 props 和其他 props 分开
  // 这是 SolidJS 的最佳实践，确保响应性
  const [local, rest] = splitProps(props, ['variant', 'size', 'class', 'type']);

  return (
    <ButtonPrimitive
      // 使用 twMerge 和 cva 来智能地合并 class
      class={twMerge(
        buttonVariants({
          variant: local.variant,
          size: local.size,
          type: local.type,
        }),
        local.class
      )}
      {...rest} // 将剩余的所有 props (如 onClick, disabled 等) 传递给 Kobalte 按钮
    />
  );
};
