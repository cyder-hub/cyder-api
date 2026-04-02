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
import { useConfirmState, handleConfirm, handleCancel } from '@/lib/confirmController'

const state = useConfirmState()
</script>

<template>
  <AlertDialog :open="state.isOpen" @update:open="(val) => !val && handleCancel()">
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle>{{ state.options.title || $t('common.confirm') }}</AlertDialogTitle>
        <AlertDialogDescription v-if="state.options.description">
          {{ state.options.description }}
        </AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel @click="handleCancel">{{ state.options.cancelText || $t('common.cancel') }}</AlertDialogCancel>
        <AlertDialogAction @click="handleConfirm">{{ state.options.confirmText || $t('common.confirm') }}</AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
