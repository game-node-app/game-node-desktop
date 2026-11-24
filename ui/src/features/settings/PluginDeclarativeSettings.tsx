import React, { useEffect, useState } from 'react';
import { commands } from '../../api';
import type { PluginSettingsContext, PluginSettingsField, PluginSettingsForm, PluginSettingsReply, PluginSettingsValue } from '../../api/pluginSettings';
import { useI18n } from '../../i18n';
import { settingsCard, settingsRowDescription, settingsRowTitle } from './settingsClasses';

function localized(text: { fallback: string; translations: Record<string, string> }, locale: string) {
  return text.translations[locale] ?? text.translations[locale.split('-')[0]] ?? text.fallback;
}

function FormField({
  field, value, locale, onChange,
}: {
  field: PluginSettingsField;
  value: PluginSettingsValue;
  locale: string;
  onChange: (value: PluginSettingsValue) => void;
}) {
  const label = localized(field.label, locale);
  const help = field.help ? localized(field.help, locale) : undefined;
  let control: React.ReactNode;
  switch (field.kind.type) {
    case 'boolean':
      control = <input aria-label={label} type="checkbox" checked={value.type === 'boolean' && value.value} onChange={(event) => onChange({ type: 'boolean', value: event.currentTarget.checked })} />;
      break;
    case 'text':
      control = <input aria-label={label} className="w-full rounded-sm border border-border-subtle bg-raised px-sm py-1 text-sm" type="text" minLength={field.kind.min_length} maxLength={field.kind.max_length} value={value.type === 'text' ? value.value : ''} onChange={(event) => onChange({ type: 'text', value: event.currentTarget.value })} />;
      break;
    case 'number':
      control = <input aria-label={label} className="w-full rounded-sm border border-border-subtle bg-raised px-sm py-1 text-sm" type="number" min={field.kind.min} max={field.kind.max} step={field.kind.step} value={value.type === 'number' ? value.value : field.kind.min} onChange={(event) => onChange({ type: 'number', value: Number(event.currentTarget.value) })} />;
      break;
    case 'choice':
      control = <select aria-label={label} className="w-full rounded-sm border border-border-subtle bg-raised px-sm py-1 text-sm" value={value.type === 'choice' ? value.value : ''} onChange={(event) => onChange({ type: 'choice', value: event.currentTarget.value })}>{field.kind.options.map((option) => <option key={option.id} value={option.id}>{localized(option.label, locale)}</option>)}</select>;
      break;
  }
  return <div className="grid gap-1">
    <label className="flex items-center gap-compact text-sm">{control}<span>{label}</span></label>
    {help && <p className={settingsRowDescription}>{help}</p>}
  </div>;
}

export const PluginDeclarativeSettings: React.FC<{ providerId: string }> = ({ providerId }) => {
  const { t, locale } = useI18n();
  const [reply, setReply] = useState<PluginSettingsReply | null>(null);
  const [values, setValues] = useState<Record<string, PluginSettingsValue>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  const describe = async () => {
    setBusy(true);
    setError(false);
    try {
      const result = await commands.pluginSettingsDescribe(providerId);
      setReply(result);
      if (result.outcome.type === 'form') setValues(result.outcome.values);
    } catch { setError(true); }
    finally { setBusy(false); }
  };

  useEffect(() => { void describe(); }, [providerId]);

  const submit = async (form: PluginSettingsForm, context: PluginSettingsContext) => {
    setBusy(true);
    setError(false);
    try {
      const result = await commands.pluginSettingsSubmit(context, form.schema_revision, form.configuration_revision, values);
      setReply(result);
      if (result.outcome.type === 'form') setValues(result.outcome.values);
      if (result.outcome.type === 'revision_conflict') await describe();
    } catch { setError(true); }
    finally { setBusy(false); }
  };

  const cancel = async (context: PluginSettingsContext) => {
    setBusy(true);
    setError(false);
    try { setReply(await commands.pluginSettingsCancel(context)); }
    catch { setError(true); }
    finally { setBusy(false); }
  };

  const formReply = reply?.outcome.type === 'form' ? reply.outcome : undefined;
  const form = formReply?.form;
  return <section className={`${settingsCard} mt-compact`} aria-label={t('plugins.settings.formTitle')}>
    {form && formReply && reply && <>
      <h4 className={settingsRowTitle}>{localized(form.title, locale)}</h4>
      <div className="mt-compact grid gap-compact">
        {form.fields.map((field) => <FormField key={field.id} field={field} locale={locale} value={values[field.id] ?? field.default} onChange={(value) => setValues((current) => ({ ...current, [field.id]: value }))} />)}
      </div>
      <div className="mt-compact flex gap-compact">
        <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={busy} onClick={() => void cancel(reply.context)}>{t('plugins.settings.cancel')}</button>
        <button type="button" className="rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white hover:brightness-110" disabled={busy} onClick={() => void submit(form, reply.context)}>{busy ? t('plugins.settings.saving') : t('plugins.settings.save')}</button>
      </div>
    </>}
    {!form && <p className={settingsRowDescription} role="status">{busy ? t('plugins.settings.loading') : error ? t('plugins.settings.failed') : t('plugins.settings.unavailable')}</p>}
    {error && form && <p className="mt-compact text-sm text-warning" role="alert">{t('plugins.settings.failed')}</p>}
    {reply?.outcome.type === 'applied' && <p className={settingsRowDescription} role="status">{t('plugins.settings.saved')}</p>}
    {reply?.outcome.type === 'failed' && <p className="mt-compact text-sm text-warning" role="alert">{t('plugins.settings.failed')}</p>}
    {reply?.outcome.type === 'field_errors' && Object.entries(reply.outcome.errors).map(([field, message]) => <p className="mt-compact text-sm text-warning" role="alert" key={field}>{localized(message, locale)}</p>)}
  </section>;
};
