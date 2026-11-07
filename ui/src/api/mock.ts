/// <reference types="vite/client" />
/** This switch is evaluated at Vite startup and is never enabled in production builds. */
export const mockEnabled = import.meta.env.DEV && import.meta.env.VITE_UI_MOCK === '1';

export async function getMockHost() {
  return (await import('../mock/host')).mockHost;
}
