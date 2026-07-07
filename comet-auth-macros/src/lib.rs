use proc_macro::TokenStream;

use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{FnArg, Ident, ItemFn, LitStr, Pat, Result, Token, Type, parse_macro_input, parse_quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardKind {
    Required,
    Optional,
    Authorized,
}

#[derive(Debug, Default)]
struct RequiresAuthArgs {
    optional: bool,
    roles: Vec<LitStr>,
    permissions: Vec<LitStr>,
    scopes: Vec<LitStr>,
}

impl RequiresAuthArgs {
    fn has_authorization_policy(&self) -> bool {
        !self.roles.is_empty() || !self.permissions.is_empty() || !self.scopes.is_empty()
    }
}

enum RequiresAuthArg {
    Optional,
    Role(LitStr),
    Permission(LitStr),
    Scope(LitStr),
}

impl Parse for RequiresAuthArg {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let name: Ident = input.parse()?;
        match name.to_string().as_str() {
            "optional" => Ok(Self::Optional),
            "role" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Role(input.parse()?))
            }
            "permission" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Permission(input.parse()?))
            }
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
                RequiresAuthArg::Role(role) => args.roles.push(role),
                RequiresAuthArg::Permission(permission) => args.permissions.push(permission),
                RequiresAuthArg::Scope(scope) => args.scopes.push(scope),
            }
        }

        if args.optional && args.has_authorization_policy() {
            return Err(syn::Error::new(
                Span::call_site(),
                "`optional` cannot be combined with authorization policies",
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
    } else if args.has_authorization_policy() {
        GuardKind::Authorized
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
        (GuardKind::Required | GuardKind::Authorized, Some(GuardKind::Optional)) => {
            return Err(syn::Error::new(
                item.sig.ident.span(),
                "`requires_auth` requires an authenticated session, but this route already takes `OptionalAuthSession`",
            ));
        }
        (GuardKind::Optional, Some(GuardKind::Required | GuardKind::Authorized)) => {
            return Err(syn::Error::new(
                item.sig.ident.span(),
                "`requires_auth(optional)` requires `OptionalAuthSession`, but this route already takes a required auth guard",
            ));
        }
        (_, Some(_)) => {}
        (GuardKind::Required, None) => {
            item.sig.inputs.insert(0, auth_session_arg());
        }
        (GuardKind::Optional, None) => {
            item.sig.inputs.insert(0, optional_auth_session_arg());
        }
        (GuardKind::Authorized, None) => {}
    }

    let policy = if args.has_authorization_policy() {
        let policy_ident = format_ident!("__CometAuthPolicyFor{}", item.sig.ident);
        let roles = args.roles;
        let permissions = args.permissions;
        let scopes = args.scopes;
        item.sig
            .inputs
            .insert(0, authorized_session_arg(&policy_ident));

        quote! {
            #[allow(non_camel_case_types)]
            pub struct #policy_ident;

            impl ::comet_auth::RequiredAuthorization for #policy_ident {
                const REQUIREMENT: ::comet_auth::AuthorizationRequirement =
                    ::comet_auth::AuthorizationRequirement::new(
                        &[#(#roles),*],
                        &[#(#permissions),*],
                        &[#(#scopes),*],
                    );
            }
        }
    } else {
        quote!()
    };

    Ok(quote! {
        #policy
        #item
    })
}

fn auth_guard_kind(ty: &Type) -> Option<GuardKind> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let ident = &type_path.path.segments.last()?.ident;
    match ident.to_string().as_str() {
        "AuthSession" => Some(GuardKind::Required),
        "OptionalAuthSession" => Some(GuardKind::Optional),
        "AuthorizedSession" => Some(GuardKind::Authorized),
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

fn authorized_session_arg(policy_ident: &Ident) -> FnArg {
    let pat: Pat = parse_quote!(_comet_auth_authorized_session);
    parse_quote!(#pat: ::comet_auth::AuthorizedSession<#policy_ident>)
}
