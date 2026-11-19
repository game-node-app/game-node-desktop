import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import type { PluginCompatibilityDetails, PluginTargetAssessment } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginCompatibilityReview } from './PluginCompatibilityReview';

const glibcTarget: PluginTargetAssessment['target'] = {
  os: 'linux', arch: 'x86_64', abi: 'gnu', entrypoint: 'bin/linux-gnu/plugin',
  requires: { linux: { libc: { family: 'glibc', min_version: '2.31' } } },
  tested_on: [{ distribution: 'ubuntu', version: '22.04' }],
};

function details(overrides: Partial<PluginCompatibilityDetails>): PluginCompatibilityDetails {
  return {
    state: 'compatible', ambiguous: false, os: 'ubuntu', release: '22.04', architecture: 'x86_64', abi: 'gnu',
    libc: { family: 'gnu', version: '2.35' }, emulation: 'native',
    targets: [{ target: glibcTarget, verdict: { state: 'compatible', reasons: ['requirementsSatisfied'], unknownFacts: [] } }],
    ...overrides,
  };
}

function renderReview(value: PluginCompatibilityDetails, locale = 'en') {
  return render(<I18nProvider initialLocale={locale}><PluginCompatibilityReview details={value} /></I18nProvider>);
}

describe('PluginCompatibilityReview', () => {
  it('shows a musl host against a glibc target as incompatible with the libc reason', () => {
    renderReview(details({
      state: 'incompatible', os: 'alpine', release: '3.19', libc: { family: 'musl', version: '1.2.4' }, abi: 'musl',
      targets: [{ target: glibcTarget, verdict: { state: 'incompatible', reasons: ['linuxLibcMismatch', 'abiMismatch'], unknownFacts: [] } }],
    }));
    const section = screen.getByRole('region', { name: 'Compatibility with this system' });
    expect(within(section).getAllByText('Incompatible').length).toBeGreaterThan(0);
    expect(section).toHaveTextContent('The C library family does not match (glibc and musl are not interchangeable).');
    expect(within(section).getByText('musl 1.2.4')).toBeInTheDocument();
    expect(section).toHaveTextContent('C library: glibc 2.31 or newer');
    expect(section).toHaveTextContent('No target of this package is compatible with this system, so it cannot be activated.');
    expect(section).not.toHaveTextContent('Activation is still allowed');
  });

  it('does not treat unknown mandatory facts as compatible', () => {
    renderReview(details({
      state: 'indeterminate', libc: { family: 'unknown' },
      targets: [{ target: glibcTarget, verdict: { state: 'indeterminate', reasons: ['linuxLibcUnknown'], unknownFacts: ['libc'] } }],
    }));
    const section = screen.getByRole('region', { name: 'Compatibility with this system' });
    expect(section).toHaveTextContent('Cannot be determined');
    expect(section).toHaveTextContent('The C library could not be determined.');
    expect(section).toHaveTextContent('Could not be determined: libc');
    expect(section).toHaveTextContent('cannot be activated');
    expect(section).not.toHaveTextContent('A target of this package is compatible');
  });

  it('keeps author-tested coverage separate from a compatible but untested system', () => {
    renderReview(details({ os: 'fedora', release: '40' }));
    const section = screen.getByRole('region', { name: 'Compatibility with this system' });
    expect(within(section).getAllByText('Compatible')).toHaveLength(2);
    expect(section).toHaveTextContent('Compatible with this system, but the author has not listed it as tested.');
    expect(section).toHaveTextContent('Author-tested environments (informational): ubuntu 22.04');
  });

  it('reports a tested system and a package without tested environments', () => {
    const { unmount } = renderReview(details({}));
    expect(screen.getByRole('region')).toHaveTextContent('This system matches an environment the author tested.');
    unmount();
    renderReview(details({ targets: [{ target: { ...glibcTarget, tested_on: [] }, verdict: { state: 'compatible', reasons: ['requirementsSatisfied'], unknownFacts: [] } }] }));
    expect(screen.getByRole('region')).toHaveTextContent('The author lists no tested environments.');
  });

  it('blocks activation text when several targets are compatible', () => {
    renderReview(details({ ambiguous: true, state: 'incompatible' }));
    expect(screen.getByRole('region')).toHaveTextContent('More than one target is compatible');
  });

  it('renders the same facts in pt-BR', () => {
    renderReview(details({
      state: 'incompatible', libc: { family: 'musl', version: '1.2.4' },
      targets: [{ target: glibcTarget, verdict: { state: 'incompatible', reasons: ['linuxLibcMismatch'], unknownFacts: [] } }],
    }), 'pt-BR');
    const section = screen.getByRole('region', { name: 'Compatibilidade com este sistema' });
    expect(section).toHaveTextContent('Incompatível');
    expect(section).toHaveTextContent('A família da biblioteca C não corresponde (glibc e musl não são intercambiáveis).');
    expect(within(section).getByText('musl 1.2.4')).toBeInTheDocument();
    expect(section).toHaveTextContent('Ambientes testados pelo autor (informativo): ubuntu 22.04');
  });

  it('shows pt-BR compatible-but-untested activation state', () => {
    renderReview(details({ os: 'fedora', release: '40' }), 'pt-BR');
    expect(screen.getByRole('region')).toHaveTextContent('Compatível com este sistema, mas o autor não o listou como testado.');
  });
});
