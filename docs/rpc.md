# Comet RPC

O Comet RPC escaneia rotas Rocket/Comet existentes e gera um manifesto JSON
versionado. A partir desse manifesto, o CLI gera clientes tipados para
TypeScript, Dart e Rust.

O RPC nao substitui as rotas HTTP. Guards, fairings e validacoes continuam no
servidor; o contrato gerado apenas reflete inputs, outputs e autenticacao para
clientes.

## Comandos

Gerar manifesto:

```sh
comet rpc manifest --path examples/cloudflare-worker --out .comet-rpc.json
```

Gerar clientes:

```sh
comet rpc generate --lang ts --path examples/cloudflare-worker --out src/comet-rpc.ts
comet rpc generate --lang dart --path examples/cloudflare-worker --out lib/comet_rpc.dart
comet rpc generate --lang rust --path examples/cloudflare-worker --out src/comet_rpc.rs
```

`--path` aponta para o projeto Rust alvo. Quando omitido, o diretorio atual e
usado. `--out` e opcional; sem ele, o output vai para stdout.

## Rotas Elegiveis

Uma rota entra como contrato JSON quando o scanner encontra um atributo Rocket
suportado e consegue inferir ao menos um contrato JSON de entrada ou saida.

Atributos reconhecidos:

- `#[get(...)]`
- `#[post(...)]`
- `#[put(...)]`
- `#[delete(...)]`
- `#[patch(...)]`

Entradas e saidas reconhecidas no MVP:

- path params como `/<id>` e `/<key..>`;
- query params como `?<done>&<page>`;
- body `Json<T>` vinculado a `data = "<arg>"`;
- response `Json<T>`;
- response `Result<Json<T>, E>`;
- aliases simples como `type ApiResult<T> = Result<T, ApiError>`.

Rotas sem contrato JSON ainda aparecem no manifesto quando sao detectadas, mas
com `support: "raw"` ou `support: "unsupported"`. Isso evita que uma rota suma
silenciosamente da analise.

## Autenticacao

O manifesto representa autenticacao sem expor tokens ou segredos.

Padroes detectados:

- `AuthSession`: auth obrigatoria;
- `OptionalAuthSession`: auth opcional;
- `AuthorizedSession<P>`: auth obrigatoria com policy;
- `#[comet_auth::requires_auth(...)]`: roles, permissions, scopes, resource e
  modo `any`/`all`;
- `RequiredAuthorization`: resolvido por introspeccao compilada quando o crate
  alvo compila.

Clientes gerados usam bearer token no MVP. TypeScript e Dart recebem um callback
opcional de token; Rust recebe token opcional no builder. Rotas publicas nao
tentam enviar token.

## Tipos Suportados

O scanner descobre structs publicos com campos nomeados e enums unitarios
simples usados direta ou indiretamente por rotas JSON.

Mapeamentos comuns:

- Rust primitives: `String`, `&str`, integers, floats, `bool`;
- wrappers: `Option<T>` e `Vec<T>`;
- structs locais publicos com campos publicos;
- enums unitarios locais;
- `serde(rename = "...")` em campos e variantes;
- `serde(rename_all = "...")` em structs e enums para `snake_case`,
  `camelCase`, `PascalCase`, `kebab-case` e `SCREAMING_SNAKE_CASE`;
- `serde(skip)` em campos.

Quando um tipo ainda nao tem schema estrutural descoberto, o gerador usa fallback
seguro:

- TypeScript: `unknown`;
- Dart: `Object?`;
- Rust: `serde_json::Value`.

## Limitacoes Iniciais

Ainda nao ha suporte estrutural completo para:

- forms e multipart;
- streaming, WebSocket, assets e bytes raw como cliente tipado;
- responders customizados sem `Json<T>`;
- `serde(flatten)`;
- enums `untagged`, adjacently tagged ou com payload;
- query structs;
- reexports e imports complexos para resolver mounts em todos os casos.

Nesses casos, o manifesto deve marcar a rota como `raw`/`unsupported`, emitir
warnings quando a inferencia for ambigua, ou gerar fallback de tipo no cliente.
