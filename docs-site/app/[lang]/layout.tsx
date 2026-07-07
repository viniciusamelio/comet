import type { ReactNode } from 'react';
import { RootProvider } from 'fumadocs-ui/provider/next';
import '../global.css';
import { Inter } from 'next/font/google';
import { provider } from '@/lib/layout.shared';

const inter = Inter({
  subsets: ['latin'],
});

interface Params {
  lang: string;
}

export default async function Layout({
  params,
  children,
}: {
  params: Promise<Params>;
  children: ReactNode;
}) {
  const { lang } = await params;

  return (
    <html lang={lang} className={inter.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <RootProvider i18n={provider(lang)}>{children}</RootProvider>
      </body>
    </html>
  );
}
