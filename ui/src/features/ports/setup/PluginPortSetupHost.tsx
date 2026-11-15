import { useEffect, useRef } from 'react';
import { usePortsView } from '../../../api/ports';
import { commands } from '../../../api/commands';
import { setActiveDestination } from '../../../store/uiStore';
import { closePortSetup, openPortSetup, portSetupStore } from './setupStore';

/** Bring the existing guided setup to the user, once per host-issued request. */
export const PluginPortSetupHost = () => {
  const request = usePortsView().pluginRequest;
  const opened = useRef<string | null>(null);
  useEffect(() => {
    if (!request) {
      if (opened.current && portSetupStore.state.pluginRequestId === opened.current) closePortSetup();
      opened.current = null;
      return;
    }
    if (opened.current === request.id) return;
    if (portSetupStore.state.open && portSetupStore.state.pluginRequestId !== request.id) {
      opened.current = request.id;
      void commands.portsDismissPluginSetup(request.id);
      return;
    }
    opened.current = request.id;
    setActiveDestination('ports');
    openPortSetup(request.catalogId, { pluginRequestId: request.id });
  }, [request]);
  return null;
};
