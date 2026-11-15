import {
  IconBrandSteam,
  IconDeviceGamepad2,
  IconFolder,
  type TablerIcon,
} from '@tabler/icons-react';

/**
 * Store icon map from `design-reference/icons.md`: Steam uses its brand icon,
 * the DRM-free provider a folder, and every other manifest a gamepad. Kept
 * local to the library controls so it does not collide with the sidebar's copy.
 */
export function providerIcon(id: string): TablerIcon {
  if (id === 'steam') return IconBrandSteam;
  if (id === 'manual' || id === 'drm-free') return IconFolder;
  return IconDeviceGamepad2;
}
