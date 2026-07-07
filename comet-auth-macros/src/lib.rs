use proc_macro::TokenStream;

use proc_macro2::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{FnArg, Ident, ItemFn, LitStr, Pat, Result, Token, Type, parse_macro_input, parse_quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardKind {
    Required,
    Optional,
}

#[derive(Debug, Default)]
struct RequiresAuthArgs {
    optional: bool,
    scope: Option<LitStr>,
}

enum RequiresAuthArg {
    Optional,
    Scope(LitStr),
}

impl Parse for RequiresAuthArg {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let name: Ident = input.parse()?;
        match name.to_string().as_str() {
            "optional" => Ok(Self::Optional),
            "scope" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Scope(input.parse()?))
            }
            other => Err(syn::Error::new(
                name.span(),
                format!("unsupported requires_auth policy `{other}`"),
            )),
        }
    }
}

impl Parse for RequiresAuthArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.is_empty() {
            return Ok(Self::default());
        }

        let mut args = Self::default();
        let parsed = Punctuated::<RequiresAuthArg, Token![,]>::parse_terminated(input)?;
        for arg in parsed {
            match arg {
                RequiresAuthArg::Optional => args.optional = true,
                RequiresAuthArg::Scope(scope) => {
                    if args.scope.replace(scope).is_some() {
                        return Err(syn::Error::new(
                            Span::call_site(),
                            "`scope` can only be declared once",
                        ));
                    }
                }
            }
        }

        if args.optional && args.scope.is_some() {
            return Err(syn::Error::new(
                Span::call_site(),
                "`optional` cannot be combined with `scope`",
            ));
        }

        Ok(args)
    }
}

#[proc_macro_attribute]
pub fn requires_auth(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as RequiresAuthArgs);
    let mut item = parse_macro_input!(input as ItemFn);

    match expand_requires_auth(args, &mut item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_requires_auth(
    args: RequiresAuthArgs,
    item: &mut ItemFn,
) -> Result<proc_macro2::TokenStream> {
    let desired = if args.optional {
        GuardKind::Optional
    } else {
        GuardKind::Required
    };

    let mut existing = None;
    for input in &item.sig.inputs {
        match input {
            FnArg::Receiver(receiver) => {
                return Err(syn::Error::new(
                    receiver.self_token.span,
                    "`requires_auth` can only be applied to free route handlers",
                ));
            }
            FnArg::Typed(pat_type) => {
                if let Some(kind) = auth_guard_kind(&pat_type.ty) {
                    existing = Some(kind);
                }
            }
        }
    }

    match (desired, existing) {
        (GuardKind::Required, Some(GuardKind::Optional)) => {
            return Err(syn::Error::new(
                item.sig.ident.span(),
                "`requires_auth` requires `AuthSession`, but this route already takes `OptionalAuthSession`",
            ));
        }
        (GuardKind::Optional, Some(GuardKind::Required)) => {
            return Err(syn::Error::new(
                item.sig.ident.span(),
                "`requires_auth(optional)` requires `OptionalAuthSession`, but this route already takes `AuthSession`",
            ));
        }
        (_, Some(_)) => {}
        (GuardKind::Required, None) => {
            item.sig.inputs.insert(0, auth_session_arg());
        }
        (GuardKind::Optional, None) => {
            item.sig.inputs.insert(0, optional_auth_session_arg());
        }
    }

    if let Some(scope) = args.scope {
        item.block.stmts.insert(
            0,
            parse_quote! { let _comet_auth_required_scope: &str = #scope; },
        );
    }

    Ok(quote!(#item))
}

fn auth_guard_kind(ty: &Type) -> Option<GuardKind> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let ident = &type_path.path.segments.last()?.ident;
    match ident.to_string().as_str() {
        "AuthSession" => Some(GuardKind::Required),
        "OptionalAuthSession" => Some(GuardKind::Optional),
        _ => None,
    }
}

fn auth_session_arg() -> FnArg {
    let pat: Pat = parse_quote!(_comet_auth_session);
    parse_quote!(#pat: ::comet_auth::AuthSession)
}

fn optional_auth_session_arg() -> FnArg {
    let pat: Pat = parse_quote!(_comet_auth_session);
    parse_quote!(#pat: ::comet_auth::OptionalAuthSession)
}
