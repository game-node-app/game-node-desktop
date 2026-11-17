import React from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { act, fireEvent, screen } from '@testing-library/react';
import { renderUi } from '../../../components/ui/testUtils';
import { ScrollFade } from './ScrollFade';

function Harness({ raised = false }: { raised?: boolean }) {
  const scrollerRef = React.useRef<HTMLDivElement>(null);
  return (
    <div>
      <div ref={scrollerRef} data-testid="scroller" className="overflow-y-auto">
        <div>Panel content</div>
      </div>
      <ScrollFade scrollerRef={scrollerRef} raised={raised} data-testid="fade" />
    </div>
  );
}

function setScrollMetrics(
  scroller: HTMLElement,
  { scrollHeight, clientHeight }: { scrollHeight: number; clientHeight: number },
) {
  Object.defineProperty(scroller, 'scrollHeight', {
    configurable: true,
    value: scrollHeight,
  });
  Object.defineProperty(scroller, 'clientHeight', {
    configurable: true,
    value: clientHeight,
  });
}

const originalResizeObserver = globalThis.ResizeObserver;

afterEach(() => {
  (globalThis as typeof globalThis & {
    ResizeObserver: typeof ResizeObserver;
  }).ResizeObserver = originalResizeObserver;
});

describe('ScrollFade', () => {
  it('shows while more content remains and hides at the scroll end', () => {
    renderUi(<Harness />);
    const scroller = screen.getByTestId('scroller');
    const fade = screen.getByTestId('fade');

    setScrollMetrics(scroller, { scrollHeight: 1000, clientHeight: 400 });
    scroller.scrollTop = 0;
    fireEvent.scroll(scroller);
    expect(fade).toHaveAttribute('data-visible', 'true');
    expect(fade).toHaveClass('opacity-100', 'bg-settings-fade', 'bottom-0');

    scroller.scrollTop = 600;
    fireEvent.scroll(scroller);
    expect(fade).toHaveAttribute('data-visible', 'false');
    expect(fade).toHaveClass('opacity-0');

    scroller.scrollTop = 120;
    fireEvent.scroll(scroller);
    expect(fade).toHaveAttribute('data-visible', 'true');
    expect(fade).toHaveAttribute('aria-hidden', 'true');
    expect(fade).toHaveClass('pointer-events-none');
  });

  it('sits above the unsaved-changes bar when raised', () => {
    renderUi(<Harness raised />);

    expect(screen.getByTestId('fade')).toHaveClass('bottom-[56px]');
    expect(screen.getByTestId('fade')).not.toHaveClass('bottom-0');
  });

  it('recomputes when the panel or its content resizes', () => {
    let resizeCallback: ResizeObserverCallback | undefined;
    class ResizeObserverMock {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    (globalThis as typeof globalThis & {
      ResizeObserver: typeof ResizeObserver;
    }).ResizeObserver =
      ResizeObserverMock as unknown as typeof ResizeObserver;

    renderUi(<Harness />);
    const scroller = screen.getByTestId('scroller');
    const fade = screen.getByTestId('fade');
    expect(fade).toHaveAttribute('data-visible', 'false');

    setScrollMetrics(scroller, { scrollHeight: 1200, clientHeight: 400 });
    act(() => {
      resizeCallback?.([], {} as ResizeObserver);
    });
    expect(fade).toHaveAttribute('data-visible', 'true');
  });
});
