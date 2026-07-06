import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import Image from 'next/image';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <>
          <Image src="/comet.svg" alt="Comet" width={20} height={16} />
          Comet
        </>
      ),
    },
    githubUrl: 'https://github.com/viniciusamelio/comet',
  };
}
