import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconCloudOff, IconRadar, IconWifiOff } from '@tabler/icons-react';
import { Banner } from './Banner';
import { renderUi } from './testUtils';

describe('Banner', () => {
  it.each([
    ['warning', 'alert', 'bg-warning-subtle', 'border-warning'],
    ['danger', 'alert', 'bg-danger-subtle', 'border-danger'],
    ['neutral', 'status', 'bg-surface', 'border-text-muted'],
    ['brand', 'status', 'bg-accent-subtle', 'border-brand'],
  ] as const)(
    'renders the %s tone with role=%s',
    (tone, role, background, border) => {
      renderUi(
        <Banner tone={tone} icon={IconCloudOff} title="Store problem" body="Body" />,
      );
      const banner = screen.getByRole(role);
      expect(banner).toHaveClass(background, border);
      expect(screen.getByText('Store problem')).toHaveClass('text-text-primary');
      expect(screen.getByText('Body')).toHaveClass('text-text-secondary');
    },
  );

  it('renders a small secondary action button and calls it', async () => {
    const onClick = vi.fn();
    renderUi(
      <Banner
        tone="brand"
        icon={IconRadar}
        title="3 new games discovered"
        action={{ label: 'Review', onClick }}
      />,
    );
    const action = screen.getByRole('button', { name: 'Review' });
    expect(action).toHaveClass('h-[32px]', 'bg-raised');
    await userEvent.click(action);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('renders the dismiss button with the accessible name', async () => {
    const onDismiss = vi.fn();
    renderUi(
      <Banner
        tone="neutral"
        icon={IconWifiOff}
        title="You're offline"
        onDismiss={onDismiss}
        dismissLabel="Dismiss"
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('omits dismiss when no label is supplied', () => {
    renderUi(
      <Banner tone="neutral" icon={IconWifiOff} title="Offline" onDismiss={() => undefined} />,
    );
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});
