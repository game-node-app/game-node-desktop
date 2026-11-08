import React from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultPortsView,
  createDefaultSnapshot,
  type Snapshot,
} from '../../api';
import type {
  PortDetailView,
  PortEntryView,
  PortSetupSlot,
  PortsView as PortsViewPayload,
} from '../../api/ports';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { PortsView } from './PortsView';
import { PortSetupHost, openPortSetup } from './setup';
import { resetPortSetup } from './setup/setupStore';

const NOW = 1_700_000_000;

function makeSlot(overrides: Partial<PortSetupSlot> & { id: string }): PortSetupSlot {
  return {
    label: 'A file',
    prompt: null,
    kind: 'file',
    required: true,
    extensions: [],
    contains: [],
    acceptedLabels: [],
    destination: null,
    root: null,
    argument: null,
    packageOption: null,
    ...overrides,
  };
}

function makeEntry(overrides: Partial<PortEntryView> & { catalogId: string }): PortEntryView {
  return {
    slug: overrides.catalogId,
    name: overrides.catalogId,
    games: ['A game'],
    consoles: ['PlayStation 4'],
    supportedOS: ['windows'],
    verified: true,
    prerelease: false,
    version: '1.0.0',
    guided: false,
    withdrawn: false,
    hasWindowsAsset: true,
    noAssetReason: null,
    installable: true,
    installBlockReason: null,
    art: { capsule: true, hero: true },
    project: null,
    source: null,
    license: null,
    requirements: [],
    installedRegistrationKey: null,
    ...overrides,
  };
}

const packageOption = {
  extractorId: 'ps4-pkg',
  toolName: 'Mock PKG Extractor',
  toolSourceUrl: null,
  toolLicense: 'GPL-3.0',
  titleId: 'CUSA10101',
  updateVersion: '01.04',
  requiresUpdate: true,
};

function makeEntries(): PortEntryView[] {
  return [
    makeEntry({
      catalogId: 'qcat_aster',
      name: 'Aster Vale: Revival',
      games: ['Vale of Ashes'],
      guided: true,
      license: 'GPL-3.0-or-later',
      project: { label: 'Aster Vale Revival', url: 'https://example.invalid/aster' },
      requirements: [
        makeSlot({
          id: 'game-folder',
          label: 'Aster Vale (CUSA10101) game folder',
          prompt: 'Choose the folder from your own disc.',
          kind: 'folder',
          contains: ['eboot.bin'],
          acceptedLabels: ['CUSA10101 (01.04)'],
          destination: 'CUSA10101',
          root: 'game',
          packageOption,
        }),
        makeSlot({
          id: 'settings-backup',
          label: 'Settings backup',
          required: false,
          extensions: ['.zip'],
          argument: { kind: 'flag', name: '--settings' },
        }),
      ],
    }),
    makeEntry({
      catalogId: 'qcat_beyond',
      name: 'Beyond the Pines',
      games: ['Beyond the Pines'],
      consoles: ['PlayStation 3'],
      version: '0.9.1',
    }),
    makeEntry({
      catalogId: 'qcat_emberfall',
      name: 'Emberfall: Iron Trail',
      games: ['Emberfall'],
      version: '1.0.5',
      installedRegistrationKey: 'drmfree:emberfall',
    }),
  ];
}

function makePorts(overrides: Partial<PortsViewPayload> = {}): PortsViewPayload {
  const entries = makeEntries();
  return {
    catalog: {
      state: 'ready',
      fetchedAt: Math.floor(Date.now() / 1000) - 3600,
      entryCount: entries.length,
      error: null,
    },
    entries,
    provenance: {
      'drmfree:emberfall': {
        registrationKey: 'drmfree:emberfall',
        catalogId: 'qcat_emberfall',
        slug: 'emberfall-iron-trail',
        portName: 'Emberfall: Iron Trail',
        installedVersion: '1.0.4',
        installFolder: 'D:\\Ports\\Emberfall',
        jobId: null,
        installedAt: NOW - 5000,
      },
    },
    jobs: [],
    unseenCount: 0,
    preferences: {
      view: 'grid',
      filters: { consoles: [], guided: false, verified: false, installed: false },
      lastParentFolder: null,
    },
    ...overrides,
  };
}

function detailForEntry(catalogId: string): PortDetailView {
  const entry = makeEntries().find((candidate) => candidate.catalogId === catalogId);
  if (!entry) throw new Error(`no fixture entry ${catalogId}`);
  const release = entry.version
    ? {
        version: entry.version,
        releasedAt: '2026-09-18',
        verified: entry.verified,
        withdrawn: entry.withdrawn,
        assetName: `${entry.slug}-windows.zip`,
        assetFormat: 'zip' as const,
        sizeBytes: 1_500_000_000,
        checksumVerified: entry.verified,
        requiresChecksumConfirmation: false,
        malwareFlag: null,
      }
    : null;
  return {
    catalogId,
    description: null,
    developer: 'Mock Port Works',
    license: entry.license,
    project: entry.project,
    source: entry.source,
    art: entry.art,
    requirements: entry.requirements,
    releases: release ? [release] : [],
    selectedRelease: entry.installable ? release : null,
    withdrawalReason: null,
    updateAvailable: null,
    noAssetReason: entry.noAssetReason,
  };
}

function makeSnapshot(ports: PortsViewPayload): Snapshot {
  const snapshot = createDefaultSnapshot();
  snapshot.ports = { ...createDefaultPortsView(), ...ports };
  return snapshot;
}

function renderPorts(ports = makePorts()) {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, makeSnapshot(ports));
  const view = render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider initialLocale="pt-BR">
        <PortsView />
      </I18nProvider>
    </QueryClientProvider>,
  );
  return { ...view, queryClient };
}

const OpenTrigger: React.FC<{ catalogId: string }> = ({ catalogId }) => (
  <button type="button" data-testid="open-setup" onClick={() => openPortSetup(catalogId)}>
    Open setup
  </button>
);

function renderSetup(ports = makePorts()) {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, makeSnapshot(ports));
  const view = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale="pt-BR">
          <OpenTrigger catalogId={ports.entries[0]?.catalogId ?? 'qcat_aster'} />
          <PortSetupHost />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
  return { ...view, queryClient };
}

/** A raw key is what `t()` returns when a key is missing from every catalog. */
function expectNoRawKeys(scope: ParentNode): void {
  expect(scope.textContent ?? '').not.toMatch(/\b[A-Za-z]+::/);
  for (const element of Array.from(
    scope.querySelectorAll('[aria-label], [title], [placeholder]'),
  )) {
    for (const attribute of ['aria-label', 'title', 'placeholder']) {
      const value = element.getAttribute(attribute);
      if (value) {
        expect(value, `${attribute} leaked a raw key`).not.toMatch(/\b[A-Za-z]+::/);
      }
    }
  }
}

function expectNoEnglishDefaults(scope: ParentNode, strings: string[]): void {
  const text = scope.textContent ?? '';
  for (const value of strings) {
    expect(text, `English default "${value}" leaked in pt-BR`).not.toContain(value);
  }
}

describe('PC ports in Brazilian Portuguese', () => {
  beforeEach(() => {
    resetUiStore();
    resetPortSetup();
    vi.restoreAllMocks();
    vi.spyOn(commands, 'portsDetail').mockImplementation(async (catalogId) =>
      detailForEntry(catalogId),
    );
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue();
    vi.spyOn(commands, 'portsRefresh').mockResolvedValue();
    vi.spyOn(commands, 'portsMarkSeen').mockResolvedValue();
    vi.spyOn(commands, 'openToolSource').mockResolvedValue();
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsPickSupplied').mockResolvedValue({
      slotId: 'game-folder',
      path: 'D:\\Dumps\\Aster Vale',
      status: 'accepted',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the catalog toolbar, tiles and credit in pt-BR', () => {
    const { container } = renderPorts();

    expect(screen.getByTestId('ports-search-input')).toHaveAttribute(
      'placeholder',
      'Pesquisar no catálogo de ports',
    );
    expect(screen.getByRole('button', { name: 'Configuração guiada' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Verificado' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Instalado' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Atualizar' })).toBeInTheDocument();
    expect(screen.getByTestId('ports-view-grid-btn')).toHaveTextContent('Grade');
    expect(screen.getByTestId('ports-view-list-btn')).toHaveTextContent('Lista');
    expect(screen.getByTestId('ports-credit')).toHaveTextContent(
      'Catálogo de ports do quiverlauncher.com',
    );
    expect(screen.getByTestId('ports-catalog-status')).toHaveTextContent(/Atualizado há/);
    expect(screen.getAllByTestId('port-badge-verified')[0]).toHaveTextContent(
      'Versão verificada',
    );
    expect(screen.getByTestId('port-icon-guided')).toHaveAttribute(
      'aria-label',
      'Configuração guiada',
    );
    expect(screen.getByTestId('port-icon-installed')).toHaveAttribute(
      'aria-label',
      'Instalado',
    );

    expectNoRawKeys(container);
    expectNoEnglishDefaults(container, [
      'Search the port catalog',
      'Guided setup',
      'Verified release',
      'Refresh',
      'Port catalog by quiverlauncher.com',
      'Grid',
    ]);
  });

  it('renders the detail pane in pt-BR, including an entry with no requirements', async () => {
    const { container } = renderPorts();

    fireEvent.click(screen.getByTestId('port-tile-qcat_aster'));

    const pane = await screen.findByTestId('port-detail-pane');
    expect(pane).toHaveAttribute('aria-label', 'Detalhes do port');
    expect(screen.getByTestId('port-detail-title')).toHaveTextContent('Aster Vale: Revival');
    expect(pane).toHaveTextContent('Jogo original');
    expect(await screen.findByText('Desenvolvedor')).toBeInTheDocument();
    expect(pane).toHaveTextContent('Licença');
    expect(pane).toHaveTextContent('Requisitos');
    expect(pane).toHaveTextContent('Configuração guiada');
    expect(pane).toHaveTextContent('Obrigatório');
    expect(pane).toHaveTextContent('Opcional');
    expect(pane).toHaveTextContent('Aceitos: CUSA10101 (01.04)');
    expect(pane).toHaveTextContent('Extraído com Mock PKG Extractor');
    expect(pane).toHaveTextContent('Título do pacote: CUSA10101');
    expect(pane).toHaveTextContent('A atualização 01.04 é obrigatória.');
    expect(pane).toHaveTextContent('Copiado para CUSA10101 · pasta do jogo.');
    expect(pane).toHaveTextContent('Passado como --settings');
    expect(screen.getByTestId('port-install-btn')).toHaveTextContent('Instalar');
    expect(screen.getByTestId('port-detail-close-btn')).toHaveAttribute(
      'aria-label',
      'Fechar detalhes',
    );

    // An entry with no guided slots exercises the empty requirements caption.
    fireEvent.click(screen.getByTestId('port-tile-qcat_beyond'));
    await waitFor(() =>
      expect(screen.getByTestId('port-requirements')).toHaveTextContent(
        'Este port não precisa de nenhum arquivo seu.',
      ),
    );

    expectNoRawKeys(container);
    expectNoEnglishDefaults(container, [
      'Port details',
      'Original game',
      'Requirements',
      'This port does not need any files from you.',
      'Guided setup',
      'Install',
    ]);
  });

  it('renders the guided setup modal in pt-BR without leaking keys or English', async () => {
    vi.spyOn(commands, 'portsInstall').mockResolvedValue('job-ptbr');
    renderSetup();
    act(() => openPortSetup('qcat_aster'));

    const dialog = screen.getByRole('dialog', { name: /Configurar Aster Vale: Revival/ });
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveTextContent(
      'O GameNode baixa o port. Você fornece seus próprios arquivos do jogo.',
    );
    expect(dialog).toHaveTextContent('Pasta de destino');
    await waitFor(() => expect(dialog).toHaveTextContent(/Lançado em: /));
    expect(dialog).not.toHaveTextContent('Lançado em: 2026-09-18');
    expect(dialog).toHaveTextContent('Nada escolhido ainda');
    expect(dialog).toHaveTextContent('Arquivos que você precisa fornecer');
    expect(dialog).toHaveTextContent('Obrigatório');
    expect(dialog).toHaveTextContent('Opcional');
    expect(dialog).toHaveTextContent('O port é instalado em uma nova subpasta dentro desta pasta.');
    expect(dialog).toHaveTextContent('Você pode fechar esta janela enquanto a instalação roda.');
    expect(screen.getByTestId('port-setup-start')).toHaveTextContent('Iniciar instalação');
    expect(screen.getByTestId('port-setup-start')).toBeDisabled();
    expect(screen.getByTestId('port-setup-choose-folder')).toHaveTextContent(
      'Escolher pasta',
    );
    expect(screen.getByTestId('port-setup-mode-game-folder')).toHaveTextContent(
      'Pacotes (.pkg)',
    );
    expect(screen.getByTestId('port-setup-mode-game-folder')).toHaveTextContent(
      'Pasta extraída',
    );

    // The package option is the default, so its fields are on screen immediately.
    expect(dialog).toHaveTextContent('Pacote base');
    expect(dialog).toHaveTextContent('Pacote de atualização');
    expect(dialog).toHaveTextContent('A atualização 01.04 é obrigatória.');
    expect(screen.getByTestId('port-setup-pick-game-folder-base')).toHaveTextContent(
      'Escolher pacote base',
    );
    expect(screen.getByTestId('port-setup-pick-game-folder-update')).toHaveTextContent(
      'Escolher pacote de atualização',
    );

    fireEvent.click(screen.getByRole('radio', { name: 'Pasta extraída' }));
    expect(screen.getByTestId('port-setup-pick-game-folder')).toHaveTextContent(
      'Escolher pasta',
    );
    expect(dialog).toHaveTextContent(
      'Escolha o pacote base e, quando for exigido, o pacote de atualização.',
    );

    expectNoRawKeys(document.body);
    expectNoEnglishDefaults(document.body, [
      'Parent folder',
      'Files you need to supply',
      'Start install',
      'Choose folder',
      'Packages (.pkg)',
      'Extracted folder',
      'You can close this window',
    ]);
  });
});
