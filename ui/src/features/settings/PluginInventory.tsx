/**
 * Compatibility wrapper: the redesigned Plugins topic lives in
 * `panels/PluginsPanel.tsx`. Keep this export for callers that still import the
 * inventory by its old name.
 */
export {
  PluginsPanel as PluginInventory,
  PLUGIN_INVENTORY_QUERY_KEY,
} from './panels/PluginsPanel';
