'use client';

import { useEffect, useCallback, type RefObject } from 'react';

export interface UseDialogOptions {
  /** Whether the dialog is currently open */
  open: boolean;
  /** Callback when the dialog should close (Escape key or click outside) */
  onClose: () => void;
  /** Whether to close on Escape key press (default: true) */
  closeOnEscape?: boolean;
  /** Whether to close on click outside (default: true) */
  closeOnClickOutside?: boolean;
  /** Whether closing is currently disabled (e.g., during loading) */
  disabled?: boolean;
}

/**
 * Hook to handle common dialog behaviors: Escape key and click outside.
 * 
 * @example
 * ```tsx
 * const dialogRef = useRef<HTMLDivElement>(null);
 * useDialog(dialogRef, {
 *   open: isOpen,
 *   onClose: () => setIsOpen(false),
 * });
 * ```
 */
export function useDialog(
  ref: RefObject<HTMLElement | null>,
  options: UseDialogOptions
): void {
  const {
    open,
    onClose,
    closeOnEscape = true,
    closeOnClickOutside = true,
    disabled = false,
  } = options;

  const handleClose = useCallback(() => {
    if (!disabled) {
      onClose();
    }
  }, [disabled, onClose]);

  // Handle Escape key
  useEffect(() => {
    if (!open || !closeOnEscape) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        handleClose();
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [open, closeOnEscape, handleClose]);

  // Handle click outside
  useEffect(() => {
    if (!open || !closeOnClickOutside) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        handleClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [open, closeOnClickOutside, handleClose, ref]);
}
