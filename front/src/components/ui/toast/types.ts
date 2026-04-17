import type { VariantProps } from "class-variance-authority";
import { buttonVariants } from "@/components/ui/button";

export interface ToastProps {
  class?: string;
  variant?: VariantProps<typeof buttonVariants>["variant"];
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  defaultOpen?: boolean;
  forceMount?: boolean;
  duration?: number;
}

export interface ToastActionProps {
  altText: string;
  asChild?: boolean;
  class?: string;
}

export interface ToastCloseProps {
  class?: string;
}

export interface ToastDescriptionProps {
  class?: string;
}

export interface ToastTitleProps {
  class?: string;
}
