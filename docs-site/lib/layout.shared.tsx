import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { defineI18nUI } from 'fumadocs-ui/i18n';
import Image from 'next/image';
import { i18n } from '@/lib/i18n';

export const { provider } = defineI18nUI(i18n, {
  'pt-BR': {
    displayName: 'Português',
    'Search(search dialog)': 'Buscar',
    'Search(search trigger)': 'Buscar',
    'Open Search(search trigger)(aria-label)': 'Abrir busca',
    'Close Search(search dialog)(aria-label)': 'Fechar busca',
    'No results found(search dialog)': 'Nenhum resultado encontrado',
    'On this page(table of contents)': 'Nesta página',
    'No Headings(table of contents)': 'Sem seções',
    'Table of Contents(inline table of contents)': 'Sumário',
    'Edit on GitHub(edit page)': 'Editar no GitHub',
    'Last updated on(page footer)': 'Última atualização em',
    'Next Page(pagination)': 'Próxima página',
    'Previous Page(pagination)': 'Página anterior',
    'Choose a language(language switcher)': 'Escolher idioma',
    'Choose a language(language switcher)(aria-label)': 'Escolher idioma',
    'Toggle Theme(theme switcher)(aria-label)': 'Alternar tema',
    'Light(theme switcher)(aria-label)': 'Claro',
    'Dark(theme switcher)(aria-label)': 'Escuro',
    'System(theme switcher)(aria-label)': 'Sistema',
    'Toggle Menu(mobile menu)(aria-label)': 'Alternar menu',
    'Open Sidebar(sidebar)(aria-label)': 'Abrir barra lateral',
    'Collapse Sidebar(sidebar)(aria-label)': 'Recolher barra lateral',
    'Copy Text(code block)(aria-label)': 'Copiar texto',
    'Copied Text(code block)(aria-label)': 'Texto copiado',
    'Copy Markdown(page actions)': 'Copiar Markdown',
    'View as Markdown(page actions)': 'Ver como Markdown',
    'Open(page actions)': 'Abrir',
    'Open in GitHub(page actions)': 'Abrir no GitHub',
    'Page Not Found(404 page)': 'Página não encontrada',
    'Back to Home(404 page)': 'Voltar para o início',
    'The page you are looking for might have been removed, had its name changed, or is temporarily unavailable.(404 page)':
      'A página que você está procurando pode ter sido removida, ter tido o nome alterado ou estar temporariamente indisponível.',
    'Type(type table)': 'Tipo',
    'Default(type table)': 'Padrão',
    'Prop(type table)': 'Prop',
    'Parameters(type table)': 'Parâmetros',
    'Returns(type table)': 'Retorno',
  },
  en: {
    displayName: 'English',
  },
});

export function baseOptions(locale: string): BaseLayoutProps {
  const isPtBr = locale === 'pt-BR';

  return {
    nav: {
      title: (
        <>
          <Image src="/comet.svg" alt="Comet" width={20} height={16} />
          Comet
        </>
      ),
      url: `/${locale}`,
    },
    githubUrl: 'https://github.com/viniciusamelio/comet',
    links: [
      {
        type: 'main',
        text: isPtBr ? 'Documentação' : 'Documentation',
        url: `/${locale}/docs`,
      },
    ],
  };
}
