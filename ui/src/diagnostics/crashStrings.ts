// Strings for the crash fallback. They live here rather than in the i18n catalogs so the fallback
// still renders when the i18n provider or the catalogs are what failed. The report text itself
// stays in English: it is for bug reports.

export interface CrashStringTable {
  title: string;
  pageIntro: string;
  panelIntro: string;
  whatHappened: string;
  where: string;
  area: string;
  when: string;
  notReported: string;
  errorStack: string;
  componentStack: string;
  logCount: (count: number) => string;
  reportLabel: string;
  privacyNote: string;
  copy: string;
  copied: string;
  copyFailed: string;
  retry: string;
  reload: string;
  close: string;
}

const EN: CrashStringTable = {
  title: 'Something went wrong',
  pageIntro:
    'GameNode hit an unexpected error and could not show this screen. If you need to report it, copy the report below.',
  panelIntro: 'This panel hit an unexpected error. The rest of GameNode is still running.',
  whatHappened: 'What happened',
  where: 'Where',
  area: 'Area',
  when: 'When',
  notReported: 'Not reported',
  errorStack: 'Error stack',
  componentStack: 'Component stack',
  logCount: (count) =>
    count === 1
      ? '1 recent log entry is included in the report.'
      : `${count} recent log entries are included in the report.`,
  reportLabel: 'Crash report',
  privacyNote:
    'The report includes recent app messages, which may contain file paths or game names. Review it before sharing.',
  copy: 'Copy report and logs',
  copied: 'Copied to the clipboard.',
  copyFailed: 'Could not copy automatically. Select the report text and copy it manually.',
  retry: 'Try again',
  reload: 'Reload app',
  close: 'Close',
};

const PT_BR: CrashStringTable = {
  title: 'Algo deu errado',
  pageIntro:
    'O GameNode encontrou um erro inesperado e não conseguiu mostrar esta tela. Se precisar relatar o problema, copie o relatório abaixo.',
  panelIntro:
    'Este painel encontrou um erro inesperado. O restante do GameNode continua funcionando.',
  whatHappened: 'O que aconteceu',
  where: 'Onde',
  area: 'Área',
  when: 'Quando',
  notReported: 'Não informado',
  errorStack: 'Pilha do erro',
  componentStack: 'Pilha de componentes',
  logCount: (count) =>
    count === 1
      ? '1 registro recente do app está incluído no relatório.'
      : `${count} registros recentes do app estão incluídos no relatório.`,
  reportLabel: 'Relatório de erro',
  privacyNote:
    'O relatório inclui mensagens recentes do app, que podem conter caminhos de arquivos ou nomes de jogos. Revise antes de compartilhar.',
  copy: 'Copiar relatório e registros',
  copied: 'Copiado para a área de transferência.',
  copyFailed:
    'Não foi possível copiar automaticamente. Selecione o texto do relatório e copie manualmente.',
  retry: 'Tentar novamente',
  reload: 'Recarregar app',
  close: 'Fechar',
};

/** Picks Portuguese for any `pt` locale tag, English otherwise. */
export function resolveCrashStrings(lang: string | null | undefined): CrashStringTable {
  return (lang ?? '').toLowerCase().startsWith('pt') ? PT_BR : EN;
}
