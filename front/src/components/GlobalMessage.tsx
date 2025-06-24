import { Toast, toaster } from "@kobalte/core/toast";

// This is the ID for the toast region. It's used for targeting by toaster.show()
// and will be assigned as the HTML id attribute for the Toast.Region element.
const GLOBAL_MESSAGE_REGION_ID = "global-message-region";

// Example Tailwind CSS classes for styling. Adjust if not using Tailwind.
const toastBaseClass = "flex items-center justify-between p-4 m-2 rounded-md shadow-lg text-white text-sm";
const toastSuccessClass = "bg-green-600";
const toastErrorClass = "bg-red-600";
const toastWarnClass = "bg-yellow-500 text-black"; // Yellow often needs dark text for contrast
const toastInfoClass = "bg-blue-600";

interface ToastContentProps {
  toastId: number;
  title: string;
  description?: string;
  type: 'success' | 'error' | 'warn' | 'info';
}

const ToastContent = (props: ToastContentProps) => {
  let typeClass: string;

  switch (props.type) {
    case 'success':
      typeClass = toastSuccessClass;
      break;
    case 'error':
      typeClass = toastErrorClass;
      break;
    case 'warn':
      typeClass = toastWarnClass;
      break;
    case 'info':
    default:
      typeClass = toastInfoClass;
      break;
  }

  return (
    <Toast
      toastId={props.toastId}
      class={`${toastBaseClass} ${typeClass}`}
      // Add specific accessibility attributes if needed, though Kobalte handles many.
    >
      <div class="flex items-center">
        <div class="flex-1">
          <Toast.Title class="font-semibold">{props.title}</Toast.Title>
          {props.description && (
            <Toast.Description class="mt-1">{props.description}</Toast.Description>
          )}
        </div>
      </div>
      <Toast.CloseButton class="ml-4 -mr-1 p-1 rounded-md hover:bg-white/20 focus:outline-none focus:ring-2 focus:ring-white">
        X
      </Toast.CloseButton>
    </Toast>
  );
};

const showToast = (
  title: string,
  description?: string,
  type: ToastContentProps['type'] = 'info'
) => {
  toaster.show(
    toastProps => (
      <ToastContent
        {...toastProps}
        title={title}
        description={description}
        type={type}
      />
    ),
    { region: GLOBAL_MESSAGE_REGION_ID } // Target the specific region
  );
};

export const toastController = {
  success: (title: string, description?: string) =>
    showToast(title, description, 'success'),
  error: (title: string, description?: string) =>
    showToast(title, description, 'error'),
  warn: (title: string, description?: string) =>
    showToast(title, description, 'warn'),
  info: (title: string, description?: string) =>
    showToast(title, description, 'info'),
};

export const GlobalToaster = (props: { regionId: string }) => {
  // This component renders the Toast.Region.
  // The `regionId` prop is used to set the HTML `id` attribute.
  return (
    <Toast.Region regionId={props.regionId}>
      <Toast.List
        // Example styling for the list of toasts.
        // Adjust positioning and appearance as needed.
        class="fixed bottom-4 right-4 z-[9999] flex flex-col gap-2 p-4 w-full max-w-md outline-none"
      />
    </Toast.Region>
  );
};
