<script setup lang="ts">
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import {
  handleOpenChange,
  markCancelIntent,
  markConfirmIntent,
  useConfirmState,
} from '@/lib/confirmController'

const state = useConfirmState()
</script>

<template>
  <AlertDialog :open="state.isOpen" @update:open="handleOpenChange">
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle>{{ state.options.title || $t('common.confirm') }}</AlertDialogTitle>
        <AlertDialogDescription v-if="state.options.description">
          {{ state.options.description }}
        </AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel class="w-full sm:w-auto" @click.capture="markCancelIntent">{{ state.options.cancelText || $t('common.cancel') }}</AlertDialogCancel>
        <AlertDialogAction class="w-full sm:w-auto" @click.capture="markConfirmIntent">{{ state.options.confirmText || $t('common.confirm') }}</AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
