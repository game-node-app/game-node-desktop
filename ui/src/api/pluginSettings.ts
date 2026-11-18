export interface PluginConfigurationModes {
  externalDeclared: boolean;
  externalAuthorized: boolean;
  declarativeDeclared: boolean;
  declarativeAuthorized: boolean;
}

export interface PluginConfigurationStatus {
  status: 'open' | 'focused' | 'unavailable' | 'denied' | 'timedOut' | 'failed';
  windowId: string | null;
  message: string | null;
}

export type PluginSettingsValue =
  | { type: 'boolean'; value: boolean }
  | { type: 'text'; value: string }
  | { type: 'number'; value: number }
  | { type: 'choice'; value: string };

export interface PluginSettingsContext {
  plugin_id: string;
  provider_id: string;
  session_id: string;
  profile_epoch: number;
  form_handle: string;
}

export interface PluginSettingsText {
  fallback: string;
  translations: Record<string, string>;
}

export type PluginSettingsKind =
  | { type: 'boolean' }
  | { type: 'text'; min_length: number; max_length: number }
  | { type: 'number'; min: number; max: number; step: number }
  | { type: 'choice'; options: Array<{ id: string; label: PluginSettingsText }> };

export interface PluginSettingsField {
  id: string;
  label: PluginSettingsText;
  help?: PluginSettingsText | null;
  kind: PluginSettingsKind;
  default: PluginSettingsValue;
}

export interface PluginSettingsForm {
  schema_version: number;
  form_id: string;
  schema_revision: string;
  configuration_revision: string;
  title: PluginSettingsText;
  fields: PluginSettingsField[];
}

export type PluginSettingsOutcome =
  | { type: 'form'; form: PluginSettingsForm; values: Record<string, PluginSettingsValue> }
  | { type: 'applied'; configuration_revision: string; persistence: 'persisted' | 'session_only' }
  | { type: 'field_errors'; errors: Record<string, PluginSettingsText> }
  | { type: 'revision_conflict'; configuration_revision: string }
  | { type: 'failed'; failure: string };

export interface PluginSettingsReply {
  context: PluginSettingsContext;
  outcome: PluginSettingsOutcome;
}

export type ConfigurationChoice = 'external' | 'form' | 'denied' | 'absent';

/** A declared capability is never treated as permission; only an authorized surface is selectable. */
export function selectPluginConfiguration(modes: PluginConfigurationModes): ConfigurationChoice {
  if (modes.externalDeclared && modes.externalAuthorized) return 'external';
  if (modes.declarativeDeclared && modes.declarativeAuthorized) return 'form';
  if (modes.externalDeclared || modes.declarativeDeclared) return 'denied';
  return 'absent';
}
