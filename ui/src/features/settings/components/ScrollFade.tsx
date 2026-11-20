import React from 'react';
import { cx } from '../../../components/ui/cx';

export interface ScrollFadeProps<T extends HTMLElement = HTMLElement>
  extends React.HTMLAttributes<HTMLDivElement> {
  /** The scrolling panel observed for scroll and size changes. */
  scrollerRef: React.RefObject<T | null>;
  /** Sits above the 56px unsaved-changes bar instead of the content bottom. */
  raised?: boolean;
}

/**
 * Bottom fade for the Settings panel (`components.md` §1). It is visible while
 * the scroller has content beyond its viewport and disappears at the end, so
 * the user can tell when there is more below.
 */
export function ScrollFade<T extends HTMLElement = HTMLElement>({
  scrollerRef,
  raised = false,
  className,
  ...props
}: ScrollFadeProps<T>) {
  const [visible, setVisible] = React.useState(false);

  React.useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return undefined;

    const update = () => {
      const maxScroll = scroller.scrollHeight - scroller.clientHeight;
      setVisible(maxScroll > 1 && scroller.scrollTop < maxScroll - 1);
    };

    update();
    scroller.addEventListener('scroll', update, { passive: true });
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(() => update());
    observer?.observe(scroller);
    if (scroller.firstElementChild) {
      observer?.observe(scroller.firstElementChild);
    }

    return () => {
      scroller.removeEventListener('scroll', update);
      observer?.disconnect();
    };
  }, [scrollerRef]);

  return (
    <div
      {...props}
      aria-hidden="true"
      data-visible={visible ? 'true' : 'false'}
      className={cx(
        'pointer-events-none absolute inset-x-0 h-[56px] bg-settings-fade transition-opacity',
        raised ? 'bottom-[56px]' : 'bottom-0',
        visible ? 'opacity-100' : 'opacity-0',
        className,
      )}
    />
  );
}
