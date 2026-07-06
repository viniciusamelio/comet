import Link from 'next/link';
import Image from 'next/image';

const features = [
  {
    title: 'Rocket on Workers',
    description:
      'Write ordinary Rocket route handlers — guards, responders, fairings — and run them unmodified on Cloudflare Workers via a direct in-process dispatch into a patched Rocket core. No sockets, no Hyper.',
    href: '/docs/cloudflare-adapter',
  },
  {
    title: 'Nebula ORM',
    description:
      'A D1-first ORM core: #[derive(Entity)], typed columns, deterministic SQL builders, explicit relationships, and safe migration generation — fully feature-gated and opt-in.',
    href: '/docs/nebula-orm',
  },
  {
    title: 'comet-cli',
    description:
      'Scaffold projects, generate entities and CRUD routes, drive migration generation, and run your test/release gate from one binary.',
    href: '/docs/comet-cli',
  },
];

export default function HomePage() {
  return (
    <main className="flex flex-1 flex-col">
      <section className="flex flex-col items-center gap-6 border-b border-fd-border bg-fd-secondary/30 px-6 py-24 text-center">
        <div className="flex items-center justify-center rounded-2xl bg-neutral-900 p-6 shadow-lg">
          <Image src="/comet.svg" alt="Comet" width={140} height={111} priority />
        </div>
        <h1 className="text-4xl font-bold tracking-tight sm:text-5xl">Comet</h1>
        <p className="max-w-2xl text-fd-muted-foreground sm:text-lg">
          Run <span className="font-medium text-fd-foreground">Rocket</span> routes on{' '}
          <span className="font-medium text-fd-foreground">Cloudflare Workers</span>, backed by a
          patched Rocket that compiles to <code>wasm32-unknown-unknown</code> — with an optional
          D1-first ORM and a CLI to scaffold it all.
        </p>
        <div className="flex flex-wrap items-center justify-center gap-3">
          <Link
            href="/docs"
            className="rounded-full bg-fd-primary px-5 py-2.5 text-sm font-medium text-fd-primary-foreground transition-opacity hover:opacity-90"
          >
            Get started
          </Link>
          <Link
            href="https://github.com/viniciusamelio/comet"
            className="rounded-full border border-fd-border px-5 py-2.5 text-sm font-medium transition-colors hover:bg-fd-accent"
          >
            View on GitHub
          </Link>
        </div>
      </section>

      <section className="mx-auto grid w-full max-w-5xl gap-6 px-6 py-16 sm:grid-cols-3">
        {features.map((feature) => (
          <Link
            key={feature.href}
            href={feature.href}
            className="flex flex-col gap-2 rounded-xl border border-fd-border p-6 transition-colors hover:bg-fd-accent/50"
          >
            <h2 className="font-semibold">{feature.title}</h2>
            <p className="text-sm text-fd-muted-foreground">{feature.description}</p>
          </Link>
        ))}
      </section>
    </main>
  );
}
