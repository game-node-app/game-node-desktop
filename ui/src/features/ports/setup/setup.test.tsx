import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultPortsView,
  createDefaultSnapshot,
  type Snapshot,
} from '../../../api';
import {
  portProgressQueryKey,
  type PortDetailView,
  type PortEntryView,
  type PortJobView,
  type PortReleaseView,
  type PortSetupSlot,
  type PortSuppliedPick,
} from '../../../api/ports';
import { I18nProvider } from '../../../i18n';
import { resetUiStore } from '../../../store/uiStore';
import { ToastContainer } from '../../settings/ToastContainer';
import { PortFailureDetails, PortSetupHost, openPortSetup } from './index';
import { portSetupStore, resetPortSetup } from './setupStore';

// ---------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------

function makeSlot(overrides: Partial<PortSetupSlot>): PortSetupSlot {
  return {
    id: 'slot',
    label: 'Slot',
    prompt: null,
    kind: 'folder',
    required: false,
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

function makeEntry(overrides: Partial<PortEntryView> = {}): PortEntryView {
  return {
    catalogId: 'qcat_test',
    slug: 'test-port',
    name: 'Aster Vale: Revival',
    games: ['Aster Vale'],
    consoles: ['PlayStation 4'],
    supportedOS: ['windows'],
    verified: true,
    prerelease: false,
    version: '1.4.2',
    guided: true,
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

function makeRelease(overrides: Partial<PortReleaseView> = {}): PortReleaseView {
  return {
    version: '1.4.2',
    releasedAt: '2026-09-18',
    verified: true,
    withdrawn: false,
    assetName: 'test-windows.zip',
    assetFormat: 'zip',
    sizeBytes: 1_000_000,
    checksumVerified: true,
    requiresChecksumConfirmation: false,
    malwareFlag: null,
    ...overrides,
  };
}

function makeDetail(
  entry: PortEntryView,
  overrides: Partial<PortDetailView> = {},
): PortDetailView {
  return {
    catalogId: entry.catalogId,
    description: null,
    developer: null,
    license: null,
    project: null,
    source: null,
    art: entry.art,
    requirements: entry.requirements,
    releases: [makeRelease()],
    selectedRelease: makeRelease(),
    withdrawalReason: null,
    updateAvailable: null,
    noAssetReason: null,
    ...overrides,
  };
}

function makeJob(overrides: Partial<PortJobView>): PortJobView {
  return {
    jobId: 'job-1',
    catalogId: 'qcat_test',
    slug: 'test-port',
    name: 'Aster Vale: Revival',
    phase: 'downloading',
    slotId: null,
    outcome: null,
    failure: null,
    interruptible: true,
    retryable: false,
    seen: false,
    startedAt: 1,
    finishedAt: null,
    ...overrides,
  };
}

function makeSnapshot(
  entries: PortEntryView[],
  jobs: PortJobView[] = [],
  lastParentFolder: string | null = null,
): Snapshot {
  const snapshot = createDefaultSnapshot();
  snapshot.ports = {
    ...createDefaultPortsView(),
    catalog: {
      state: 'ready',
      fetchedAt: 1,
      entryCount: entries.length,
      error: null,
    },
    entries,
    jobs,
    unseenCount: jobs.filter((job) => !job.seen).length,
    preferences: {
      ...createDefaultPortsView().preferences,
      lastParentFolder,
    },
  };
  return snapshot;
}

const OpenTrigger: React.FC<{ catalogId: string }> = ({ catalogId }) => (
  <button
    type="button"
    data-testid="open-setup"
    onClick={() => openPortSetup(catalogId)}
  >
    Open setup
  </button>
);

function renderHost(snapshot: Snapshot, locale = 'en') {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

  const result = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>
          <OpenTrigger catalogId={snapshot.ports.entries[0]?.catalogId ?? 'qcat_test'} />
          <PortSetupHost />
          <ToastContainer />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );

  return { ...result, queryClient };
}

function clickTestId(testId: string) {
  fireEvent.click(screen.getByTestId(testId));
}

// ---------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------

describe('Port guided setup', () => {
  beforeEach(() => {
    resetUiStore();
    resetPortSetup();
    vi.restoreAllMocks();
  });

  it('blocks Start on a required slot, lets an optional slot be skipped, and sends the request', async () => {
    const required = makeSlot({
      id: 'game-folder',
      label: 'Aster Vale game folder',
      kind: 'folder',
      required: true,
      contains: ['eboot.bin'],
      acceptedLabels: ['CUSA10101 (01.04)'],
      destination: 'CUSA10101',
      root: 'game',
    });
    const optional = makeSlot({
      id: 'settings',
      label: 'Settings backup',
      kind: 'file',
      required: false,
      extensions: ['.zip'],
      argument: { kind: 'flag', name: '--settings' },
    });
    const entry = makeEntry({ requirements: [required, optional] });

    const pickParent = vi
      .spyOn(commands, 'portsPickParent')
      .mockResolvedValue('D:\\Ports');
    const setPreferences = vi
      .spyOn(commands, 'portsSetPreferences')
      .mockResolvedValue(undefined);
    const pickSupplied = vi.spyOn(commands, 'portsPickSupplied').mockResolvedValue({
      slotId: 'game-folder',
      path: 'D:\\Dumps\\Aster Vale',
      status: 'accepted',
    });
    const install = vi.spyOn(commands, 'portsInstall').mockResolvedValue('job-1');

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));

    expect(screen.getByRole('dialog', { name: /Set up Aster Vale/ })).toBeInTheDocument();
    const start = screen.getByTestId('port-setup-start');
    expect(start).toBeDisabled();

    // Parent folder first: the required slot still blocks Start.
    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-folder-value')).toHaveTextContent(
        'D:\\Ports',
      ),
    );
    expect(pickParent).toHaveBeenCalledWith(null);
    expect(setPreferences).toHaveBeenCalledWith({ lastParentFolder: 'D:\\Ports' });
    expect(start).toBeDisabled();
    // A fresh form does not report missing requirements until the user touches a source.
    expect(screen.queryByTestId('port-setup-missing-required')).toBeNull();

    // Choose the required folder; the optional file stays empty.
    clickTestId('port-setup-pick-game-folder');
    await waitFor(() => expect(pickSupplied).toHaveBeenCalledTimes(1));
    expect(pickSupplied.mock.calls[0][0]).toMatchObject({
      slotId: 'game-folder',
      kind: 'folder',
      extensions: [],
    });
    await waitFor(() => expect(start).not.toBeDisabled());
    expect(screen.queryByTestId('port-setup-missing-required')).toBeNull();

    fireEvent.click(start);
    await waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    expect(install.mock.calls[0][0]).toMatchObject({
      catalogId: entry.catalogId,
      parentFolder: 'D:\\Ports',
      supplied: [{ slotId: 'game-folder', path: 'D:\\Dumps\\Aster Vale' }],
    });
    expect(portSetupStore.state.open).toBe(false);
  });

  it('shows rejection messages with the accepted versions', async () => {
    const required = makeSlot({
      id: 'game-folder',
      label: 'Aster Vale game folder',
      kind: 'folder',
      required: true,
      acceptedLabels: ['CUSA10101 (01.04)'],
    });
    const entry = makeEntry({ requirements: [required] });

    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);
    vi.spyOn(commands, 'portsPickSupplied').mockResolvedValue({
      slotId: 'game-folder',
      path: 'D:\\Dumps\\Wrong',
      status: {
        rejected: {
          message: 'Expected CUSA10101 version 01.04.',
          acceptedLabels: ['CUSA10101 (01.04)'],
          packagesFound: false,
        },
      },
    });

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));
    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-folder-value')).toHaveTextContent('D:\\Ports'),
    );
    clickTestId('port-setup-pick-game-folder');

    const rejection = await screen.findByTestId('port-setup-rejected-game-folder');
    expect(rejection).toHaveTextContent('Expected CUSA10101 version 01.04.');
    expect(rejection).toHaveTextContent('Accepted versions: CUSA10101 (01.04)');
    expect(screen.getByTestId('port-setup-start')).toBeDisabled();
  });

  it('points at the package option when the chosen folder holds packages', async () => {
    const required = makeSlot({
      id: 'game-package',
      label: 'Aster Vale game folder',
      kind: 'folder',
      required: true,
      contains: ['eboot.bin'],
      packageOption: {
        extractorId: 'ps4-pkg',
        toolName: 'Mock PKG Extractor',
        toolSourceUrl: null,
        toolLicense: 'GPL-3.0',
        titleId: null,
        updateVersion: '01.04',
        requiresUpdate: true,
      },
    });
    const entry = makeEntry({ requirements: [required] });

    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);
    vi.spyOn(commands, 'portsPickSupplied').mockResolvedValue({
      slotId: 'game-package',
      path: 'D:\\Dumps\\pkgs',
      status: {
        rejected: {
          message: 'the folder D:\\Dumps\\pkgs does not contain eboot.bin',
          acceptedLabels: [],
          packagesFound: true,
        },
      },
    });

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));

    fireEvent.click(screen.getByRole('radio', { name: 'Extracted folder' }));
    clickTestId('port-setup-pick-game-package');

    const rejection = await screen.findByTestId('port-setup-rejected-game-package');
    expect(rejection).toHaveTextContent(
      'That folder contains game packages. Choose Packages (.pkg) above',
    );
    expect(rejection).not.toHaveTextContent('does not contain eboot.bin');
    expect(screen.getByTestId('port-setup-start')).toBeDisabled();
  });

  it('words a blocked install from the typed plan problem', async () => {
    const entry = makeEntry({ requirements: [] });
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);
    vi.spyOn(commands, 'portsInstall').mockRejectedValue({
      code: 'installBlocked',
      message: { code: 'targetNotEmpty', target: 'D:\\Ports\\Bloodborne' },
    });

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));
    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-start')).not.toBeDisabled(),
    );
    fireEvent.click(screen.getByTestId('port-setup-start'));

    const error = await screen.findByTestId('port-setup-start-error');
    expect(error).toHaveTextContent(
      'That install folder already contains files. Choose another parent folder and try again.',
    );
  });

  it('shows hashing progress keyed by the setup session id while a pick is validated', async () => {
    const required = makeSlot({
      id: 'game-folder',
      label: 'Aster Vale game folder',
      kind: 'folder',
      required: true,
    });
    const entry = makeEntry({ requirements: [required] });

    let resolvePick: ((value: PortSuppliedPick) => void) | undefined;
    const pickSupplied = vi
      .spyOn(commands, 'portsPickSupplied')
      .mockImplementation(
        () =>
          new Promise<PortSuppliedPick | null>((resolve) => {
            resolvePick = resolve;
          }),
      );
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);

    const { queryClient } = renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));
    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-folder-value')).toHaveTextContent('D:\\Ports'),
    );

    clickTestId('port-setup-pick-game-folder');
    const progressRegion = await screen.findByTestId('port-setup-progress-game-folder');
    expect(progressRegion).toHaveTextContent('Hashing the selection…');

    const setupId = pickSupplied.mock.calls[0][0].setupId;
    act(() => {
      queryClient.setQueryData(portProgressQueryKey(setupId), {
        jobId: setupId,
        phase: 'hashing',
        slotId: 'game-folder',
        bytesDone: 50,
        bytesTotal: 100,
      });
    });
    await waitFor(() => expect(progressRegion).toHaveTextContent('50%'));

    act(() => {
      resolvePick?.({
        slotId: 'game-folder',
        path: 'D:\\Dumps\\Aster Vale',
        status: 'accepted',
      });
    });
    await waitFor(() =>
      expect(
        screen.queryByTestId('port-setup-progress-game-folder'),
      ).not.toBeInTheDocument(),
    );
  });

  it('gates Start behind the unverified checksum and malware confirmations', async () => {
    const entry = makeEntry({ requirements: [] });
    vi.spyOn(commands, 'portsDetail').mockResolvedValue(
      makeDetail(entry, {
        selectedRelease: makeRelease({
          checksumVerified: false,
          requiresChecksumConfirmation: true,
          malwareFlag: 'Trojan.Win32.MockPort',
        }),
      }),
    );
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);
    const install = vi.spyOn(commands, 'portsInstall').mockResolvedValue('job-1');

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));

    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-folder-value')).toHaveTextContent('D:\\Ports'),
    );

    const start = screen.getByTestId('port-setup-start');
    expect(start).toBeDisabled();
    expect(screen.getByTestId('port-setup-checksum-gate')).toBeVisible();
    expect(screen.getByTestId('port-setup-malware-gate')).toHaveTextContent(
      'Trojan.Win32.MockPort',
    );

    fireEvent.click(screen.getByTestId('port-setup-accept-unverified'));
    expect(start).toBeDisabled();

    fireEvent.click(screen.getByTestId('port-setup-accept-malware'));
    await waitFor(() => expect(start).not.toBeDisabled());

    fireEvent.click(start);
    await waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    expect(install.mock.calls[0][0]).toMatchObject({
      acceptUnverifiedChecksum: true,
      acceptMalwareFlag: true,
    });
  });

  it('blocks a withdrawn entry', async () => {
    const entry = makeEntry({
      withdrawn: true,
      installable: false,
      installBlockReason: 'withdrawn',
    });
    vi.spyOn(commands, 'portsDetail').mockResolvedValue(
      makeDetail(entry, {
        selectedRelease: makeRelease({ withdrawn: true }),
        withdrawalReason: 'The author withdrew this release upstream.',
      }),
    );
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));

    expect(screen.getByTestId('port-setup-withdrawn')).toBeVisible();
    expect(screen.getByTestId('port-setup-blocked')).toHaveTextContent(
      'This port was withdrawn upstream.',
    );
    expect(screen.getByTestId('port-setup-start')).toBeDisabled();
  });

  it('uses the package option for a slot and requires the update package', async () => {
    const required = makeSlot({
      id: 'game-package',
      label: 'Aster Vale packages',
      kind: 'folder',
      required: true,
      packageOption: {
        extractorId: 'ps4-pkg',
        toolName: 'Mock PKG Extractor',
        toolSourceUrl: null,
        toolLicense: 'GPL-3.0',
        titleId: 'CUSA10101',
        updateVersion: '01.04',
        requiresUpdate: true,
      },
    });
    const entry = makeEntry({ requirements: [required] });
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);
    vi.spyOn(commands, 'portsPickSupplied').mockImplementation(async (request) => ({
      slotId: request.slotId,
      path: request.update ? 'D:\\Dumps\\update.pkg' : 'D:\\Dumps\\base.pkg',
      status: 'accepted',
    }));
    const install = vi.spyOn(commands, 'portsInstall').mockResolvedValue('job-1');

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId));
    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-folder-value')).toHaveTextContent('D:\\Ports'),
    );

    // A slot with a package option opens in package mode.
    expect(screen.getByTestId('port-setup-pick-game-package-base')).toBeVisible();
    expect(screen.getByTestId('port-setup-mode-game-package')).toBeVisible();

    // The extracted folder stays available; switching clears the old source.
    fireEvent.click(screen.getByRole('radio', { name: 'Extracted folder' }));
    expect(screen.getByTestId('port-setup-pick-game-package')).toBeVisible();
    expect(screen.queryByTestId('port-setup-pick-game-package-base')).toBeNull();
    expect(screen.getByTestId('port-setup-start')).toBeDisabled();

    fireEvent.click(screen.getByRole('radio', { name: 'Packages (.pkg)' }));
    clickTestId('port-setup-pick-game-package-base');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-pick-game-package-update')).toBeVisible(),
    );
    expect(screen.getByTestId('port-setup-start')).toBeDisabled();

    clickTestId('port-setup-pick-game-package-update');
    await waitFor(() => expect(screen.getByTestId('port-setup-start')).not.toBeDisabled());
    fireEvent.click(screen.getByTestId('port-setup-start'));

    await waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    expect(install.mock.calls[0][0].supplied).toEqual([
      { slotId: 'game-package', path: 'D:\\Dumps\\base.pkg', package: true },
      {
        slotId: 'game-package',
        path: 'D:\\Dumps\\update.pkg',
        package: true,
        update: true,
      },
    ]);
  });

  it('closes on Escape and restores focus to the trigger', async () => {
    const entry = makeEntry({ requirements: [] });
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);

    renderHost(makeSnapshot([entry]));
    const trigger = screen.getByTestId('open-setup');
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    fireEvent.click(trigger);
    const dialog = screen.getByRole('dialog', { name: /Set up Aster Vale/ });
    expect(dialog).toBeInTheDocument();
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));

    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: /Set up Aster Vale/ })).toBeNull(),
    );
    expect(portSetupStore.state.open).toBe(false);
    expect(document.activeElement).toBe(trigger);
  });

  it('retries from the stored plan without requiring the slots again', async () => {
    const required = makeSlot({
      id: 'game-folder',
      label: 'Aster Vale game folder',
      kind: 'folder',
      required: true,
    });
    const entry = makeEntry({ requirements: [required] });
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);
    const install = vi.spyOn(commands, 'portsInstall').mockResolvedValue('job-2');

    renderHost(makeSnapshot([entry]));
    act(() => openPortSetup(entry.catalogId, { retryJobId: 'job-old' }));

    expect(screen.getByTestId('port-setup-retry-note')).toHaveTextContent('reused');
    expect(screen.getByTestId('port-setup-start')).toHaveTextContent('Retry install');
    // The stored plan's supplied files are reused, so its slot pickers are not shown.
    expect(screen.queryByTestId('port-setup-slot-game-folder')).toBeNull();

    clickTestId('port-setup-choose-folder');
    await waitFor(() =>
      expect(screen.getByTestId('port-setup-start')).not.toBeDisabled(),
    );
    fireEvent.click(screen.getByTestId('port-setup-start'));

    await waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    expect(install.mock.calls[0][0]).toMatchObject({
      retryJobId: 'job-old',
      supplied: [],
    });
  });

  it('renders the longer pt-BR labels', async () => {
    const entry = makeEntry({ requirements: [] });
    vi.spyOn(commands, 'portsPickParent').mockResolvedValue('D:\\Ports');
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue(undefined);

    renderHost(makeSnapshot([entry]), 'pt-BR');
    act(() => openPortSetup(entry.catalogId));

    expect(screen.getByRole('dialog', { name: /Configurar Aster Vale/ })).toBeInTheDocument();
    expect(screen.getByText('Pasta de destino')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Iniciar instalação' })).toBeDisabled();
  });
});

describe('Port job results', () => {
  beforeEach(() => {
    resetUiStore();
    resetPortSetup();
    vi.restoreAllMocks();
  });

  it('toasts a completed job once per result transition', async () => {
    const entry = makeEntry();
    const running = makeJob({});
    const { queryClient } = renderHost(makeSnapshot([entry], [running]));

    expect(screen.queryByText(/was installed\./)).toBeNull();

    const completed = makeJob({
      phase: 'completed',
      outcome: 'completed',
      interruptible: false,
      finishedAt: 5,
    });
    act(() => {
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, makeSnapshot([entry], [completed]));
    });

    const toast = await screen.findByText('Aster Vale: Revival was installed.');
    expect(toast).toBeInTheDocument();

    // A later snapshot carrying the same result must not repeat the toast.
    act(() => {
      queryClient.setQueryData(
        SNAPSHOT_QUERY_KEY,
        makeSnapshot([entry], [{ ...completed }]),
      );
    });
    expect(screen.getAllByText('Aster Vale: Revival was installed.')).toHaveLength(1);
  });

  it('announces a failure, shows the tool output and retries the stored plan', async () => {
    const entry = makeEntry();
    const running = makeJob({});
    const { queryClient } = renderHost(makeSnapshot([entry], [running]));

    const failed = makeJob({
      phase: 'failed',
      outcome: 'failed',
      interruptible: false,
      retryable: true,
      finishedAt: 9,
      failure: {
        message: 'The extraction tool could not read the package.',
        toolOutput: 'pkg-extract: encrypted package (CUSA10101)',
      },
    });
    act(() => {
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, makeSnapshot([entry], [failed]));
    });

    expect(
      await screen.findByText('Aster Vale: Revival could not be installed.'),
    ).toBeInTheDocument();
    const details = await screen.findByTestId('port-failure-details');
    expect(details).toHaveTextContent('The extraction tool could not read the package.');
    expect(screen.getByTestId('port-failure-output')).toHaveTextContent(
      'pkg-extract: encrypted package (CUSA10101)',
    );

    clickTestId('port-failure-retry');
    await waitFor(() => expect(portSetupStore.state.open).toBe(true));
    expect(portSetupStore.state.retryJobId).toBe('job-1');
    expect(await screen.findByTestId('port-setup-retry-note')).toBeVisible();
    expect(screen.getByTestId('port-setup-start')).toHaveTextContent('Retry install');
  });

  it('follows a completed install that needs an executable', async () => {
    const entry = makeEntry();
    const running = makeJob({});
    const { queryClient } = renderHost(makeSnapshot([entry], [running]));
    const completed = makeJob({
      phase: 'completed',
      outcome: 'completed',
      interruptible: false,
      finishedAt: 5,
      needsExecutable: true,
    });
    const pickExecutable = vi
      .spyOn(commands, 'portsPickExecutable')
      .mockResolvedValue('D:\\Ports\\test-port\\port.exe');

    act(() => {
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, makeSnapshot([entry], [completed]));
    });

    expect(
      await screen.findByText('Aster Vale: Revival was installed.'),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole('dialog', { name: 'Choose the executable' }),
    ).toBeInTheDocument();

    clickTestId('port-pick-executable');
    await waitFor(() => expect(pickExecutable).toHaveBeenCalledWith('job-1'));
    await waitFor(() =>
      expect(screen.queryByTestId('port-pick-executable')).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByText('Executable chosen: D:\\Ports\\test-port\\port.exe'),
    ).toBeInTheDocument();
  });

  it('announces an interrupted job but not a cancelled one', async () => {
    const entry = makeEntry();
    const first = makeJob({ jobId: 'job-cancelled' });
    const second = makeJob({ jobId: 'job-interrupted' });
    const { queryClient } = renderHost(makeSnapshot([entry], [first, second]));

    act(() => {
      queryClient.setQueryData(
        SNAPSHOT_QUERY_KEY,
        makeSnapshot(
          [entry],
          [
            makeJob({
              jobId: 'job-cancelled',
              phase: 'cancelled',
              outcome: 'cancelled',
              interruptible: false,
              retryable: true,
              finishedAt: 3,
            }),
            makeJob({
              jobId: 'job-interrupted',
              phase: 'interrupted',
              outcome: 'interrupted',
              interruptible: false,
              retryable: true,
              finishedAt: 4,
              failure: {
                message: 'The application stopped before the install finished.',
                toolOutput: null,
              },
            }),
          ],
        ),
      );
    });

    expect(
      await screen.findByText('Aster Vale: Revival was interrupted.'),
    ).toBeInTheDocument();
    expect(screen.queryByText('Aster Vale: Revival could not be installed.')).toBeNull();
    expect(
      await screen.findByText('The application stopped before the install finished.'),
    ).toBeInTheDocument();
  });

  it('does not re-announce results that were already terminal when PC ports opened', () => {
    const entry = makeEntry();
    const completed = makeJob({
      phase: 'completed',
      outcome: 'completed',
      interruptible: false,
      finishedAt: 5,
    });

    renderHost(makeSnapshot([entry], [completed]));

    expect(screen.queryByText('Aster Vale: Revival was installed.')).toBeNull();
    expect(screen.queryByTestId('port-failure-details')).toBeNull();
  });

  it('copies the failure details', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });

    const failed = makeJob({
      phase: 'failed',
      outcome: 'failed',
      failure: {
        message: 'The downloaded archive could not be verified.',
        toolOutput: '7z: CRCs don\u2019t match',
      },
    });

    render(
      <I18nProvider initialLocale="en">
        <PortFailureDetails job={failed} />
      </I18nProvider>,
    );

    expect(screen.getByTestId('port-failure-output')).toHaveTextContent('CRCs');
    fireEvent.click(screen.getByTestId('port-failure-copy'));
    await waitFor(() =>
      expect(screen.getByText('Copied to the clipboard.')).toBeInTheDocument(),
    );
    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining('The downloaded archive could not be verified.'),
    );
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining('CRCs'));
  });
});
