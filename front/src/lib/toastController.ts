import { toast } from "@/components/ui/toast";

export const toastController = {
  success: (title: string, description?: string) =>
    toast({
      title,
      description,
      variant: "default",
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
      class: "bg-yellow-500 text-black",
    }),
  info: (title: string, description?: string) =>
    toast({
      title,
      description,
      variant: "default",
    }),
};
