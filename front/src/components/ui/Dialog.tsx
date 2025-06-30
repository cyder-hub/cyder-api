import { Dialog as DialogPrimitive } from '@kobalte/core/dialog';
import type { Component, ComponentProps } from 'solid-js';
import { splitProps } from 'solid-js';
import { twMerge } from 'tailwind-merge';
// 移除: import { IoClose } from 'solid-icons/io'

// 只做转发，不添加样式
const DialogRoot = DialogPrimitive;
const DialogTrigger = DialogPrimitive.Trigger;

// Portal包裹Content
const DialogPortal: Component<ComponentProps<typeof DialogPrimitive.Portal>> = (props) => {
    return <DialogPrimitive.Portal {...props} />;
};

// 遮罩层
const DialogOverlay: Component<ComponentProps<typeof DialogPrimitive.Overlay>> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return (
        <DialogPrimitive.Overlay
            class={twMerge(
                "fixed inset-0 z-50 bg-black/80 data-[expanded]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[expanded]:fade-in-0",
                local.class
            )}
            {...rest}
        />
    )
}

// Dialog 内容区
const DialogContent: Component<ComponentProps<typeof DialogPrimitive.Content>> = (props) => {
  const [local, rest] = splitProps(props, ['class', 'children']);
  return (
    <DialogPortal>
      <DialogOverlay />
      <DialogPrimitive.Content
        class={twMerge(
          'fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 border bg-white p-6 shadow-lg duration-200 data-[expanded]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[expanded]:fade-in-0 data-[closed]:zoom-out-95 data-[expanded]:zoom-in-95 data-[closed]:slide-out-to-left-1/2 data-[closed]:slide-out-to-top-[48%] data-[expanded]:slide-in-from-left-1/2 data-[expanded]:slide-in-from-top-[48%] sm:rounded-lg',
          local.class
        )}
        {...rest}
      >
        {local.children}
        <DialogPrimitive.CloseButton class="absolute right-4 top-4 rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none">
          {/* 使用内联 SVG 替换 solid-icons */}
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
            class="h-4 w-4"
          >
            <path d="M18 6 6 18" />
            <path d="M6 6l12 12" />
          </svg>
          <span class="sr-only">Close</span>
        </DialogPrimitive.CloseButton>
      </DialogPrimitive.Content>
    </DialogPortal>
  );
};

// Dialog 头部容器
const DialogHeader: Component<ComponentProps<"div">> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return <div class={twMerge("flex flex-col space-y-1.5 text-center sm:text-left", local.class)} {...rest} />;
}

// Dialog 底部容器
const DialogFooter: Component<ComponentProps<"div">> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return <div class={twMerge("flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2", local.class)} {...rest} />;
}

// Dialog 标题
const DialogTitle: Component<ComponentProps<typeof DialogPrimitive.Title>> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return <DialogPrimitive.Title class={twMerge("text-lg font-semibold leading-none tracking-tight", local.class)} {...rest} />;
}

// Dialog 描述
const DialogDescription: Component<ComponentProps<typeof DialogPrimitive.Description>> = (props) => {
    const [local, rest] = splitProps(props, ["class"]);
    return <DialogPrimitive.Description class={twMerge("text-sm text-gray-500", local.class)} {...rest} />;
}

export {
  DialogRoot,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
};
