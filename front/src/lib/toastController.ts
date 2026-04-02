import { toast } from "@/components/ui/toast";

export const toastController = {
  success: (title: string, description?: string) =>
    toast({
      title,
      description,
      variant: "default",
      class: "border-gray-900 bg-gray-900 text-white",
    }),
  error: (title: string, description?: string) =>
    toast({
      title,
      description,
      variant: "destructive",
    }),
  warn: (title: string, description?: string) =>
    toast({
      title,
      description,
      variant: "default",
      class: "border-gray-400 bg-gray-100 text-gray-900",
    }),
  info: (title: string, description?: string) =>
    toast({
      title,
      description,
      variant: "default",
      class: "border-gray-200 bg-white text-gray-900",
    }),
};
