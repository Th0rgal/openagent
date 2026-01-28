import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useRef } from 'react';
import { useDialog } from './use-dialog';

describe('useDialog', () => {
  let addEventListenerSpy: ReturnType<typeof vi.spyOn>;
  let removeEventListenerSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    addEventListenerSpy = vi.spyOn(document, 'addEventListener');
    removeEventListenerSpy = vi.spyOn(document, 'removeEventListener');
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('should not add event listeners when dialog is closed', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: false, onClose }));

    expect(addEventListenerSpy).not.toHaveBeenCalledWith('keydown', expect.any(Function));
    expect(addEventListenerSpy).not.toHaveBeenCalledWith('mousedown', expect.any(Function));
  });

  it('should add event listeners when dialog is open', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: true, onClose }));

    expect(addEventListenerSpy).toHaveBeenCalledWith('keydown', expect.any(Function));
    expect(addEventListenerSpy).toHaveBeenCalledWith('mousedown', expect.any(Function));
  });

  it('should call onClose when Escape key is pressed', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: true, onClose }));

    const keydownHandler = addEventListenerSpy.mock.calls.find(
      (call) => call[0] === 'keydown'
    )?.[1] as EventListener;

    keydownHandler(new KeyboardEvent('keydown', { key: 'Escape' }));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('should not call onClose when other keys are pressed', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: true, onClose }));

    const keydownHandler = addEventListenerSpy.mock.calls.find(
      (call) => call[0] === 'keydown'
    )?.[1] as EventListener;

    keydownHandler(new KeyboardEvent('keydown', { key: 'Enter' }));

    expect(onClose).not.toHaveBeenCalled();
  });

  it('should call onClose when clicking outside the dialog', () => {
    const onClose = vi.fn();
    const dialogElement = document.createElement('div');
    const outsideElement = document.createElement('div');
    const ref = { current: dialogElement };

    renderHook(() => useDialog(ref, { open: true, onClose }));

    const mousedownHandler = addEventListenerSpy.mock.calls.find(
      (call) => call[0] === 'mousedown'
    )?.[1] as EventListener;

    const event = new MouseEvent('mousedown', { bubbles: true });
    Object.defineProperty(event, 'target', { value: outsideElement });
    mousedownHandler(event);

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('should not call onClose when clicking inside the dialog', () => {
    const onClose = vi.fn();
    const dialogElement = document.createElement('div');
    const insideElement = document.createElement('span');
    dialogElement.appendChild(insideElement);
    const ref = { current: dialogElement };

    renderHook(() => useDialog(ref, { open: true, onClose }));

    const mousedownHandler = addEventListenerSpy.mock.calls.find(
      (call) => call[0] === 'mousedown'
    )?.[1] as EventListener;

    const event = new MouseEvent('mousedown', { bubbles: true });
    Object.defineProperty(event, 'target', { value: insideElement });
    mousedownHandler(event);

    expect(onClose).not.toHaveBeenCalled();
  });

  it('should not call onClose when disabled', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: true, onClose, disabled: true }));

    const keydownHandler = addEventListenerSpy.mock.calls.find(
      (call) => call[0] === 'keydown'
    )?.[1] as EventListener;

    keydownHandler(new KeyboardEvent('keydown', { key: 'Escape' }));

    expect(onClose).not.toHaveBeenCalled();
  });

  it('should respect closeOnEscape option', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: true, onClose, closeOnEscape: false }));

    expect(addEventListenerSpy).not.toHaveBeenCalledWith('keydown', expect.any(Function));
  });

  it('should respect closeOnClickOutside option', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    renderHook(() => useDialog(ref, { open: true, onClose, closeOnClickOutside: false }));

    expect(addEventListenerSpy).not.toHaveBeenCalledWith('mousedown', expect.any(Function));
  });

  it('should remove event listeners on cleanup', () => {
    const onClose = vi.fn();
    const ref = { current: document.createElement('div') };

    const { unmount } = renderHook(() => useDialog(ref, { open: true, onClose }));

    unmount();

    expect(removeEventListenerSpy).toHaveBeenCalledWith('keydown', expect.any(Function));
    expect(removeEventListenerSpy).toHaveBeenCalledWith('mousedown', expect.any(Function));
  });
});
