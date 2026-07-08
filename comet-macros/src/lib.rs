use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::parse::{Parse, ParseStream};
use syn::{
    Data, DeriveInput, Expr, Field, Fields, Ident, LitBool, LitStr, Path, Result, Token, Type,
    parse_macro_input,
};

#[proc_macro_derive(Entity, attributes(nebula))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match expand_entity(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_entity(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    let ident = input.ident;
    let struct_options = StructOptions::parse(&input.attrs)?;
    let table_name = struct_options
        .table
        .unwrap_or_else(|| to_snake_case(&ident.to_string()));
    let comet = struct_options
        .comet_path
        .unwrap_or_else(|| syn::parse_quote!(::comet));

    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "Nebula Entity can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ident,
                "Nebula Entity can only be derived for structs",
            ));
        }
    };

    let mut column_names = HashSet::new();
    let mut primary_key_count = 0usize;
    let mut column_consts = Vec::new();
    let mut column_defs = Vec::new();
    let mut foreign_key_defs = Vec::new();

    for field in fields {
        let field_ident = field.ident.clone().expect("named field");
        let options = FieldOptions::parse(&field)?;
        let column_name = options.rename.unwrap_or_else(|| field_ident.to_string());

        if !column_names.insert(column_name.clone()) {
            return Err(syn::Error::new_spanned(
                field_ident,
                format!("duplicate Nebula column name `{column_name}`"),
            ));
        }

        if options.primary_key {
            primary_key_count += 1;
        }

        if options.auto_increment && !is_integer_type(&field.ty) {
            return Err(syn::Error::new_spanned(
                field.ty,
                "Nebula `auto`/`auto_increment` requires an integer field type",
            ));
        }

        let sql_type = sql_type_tokens(&field.ty, &comet)?;
        let const_ident = format_ident!("{}", to_upper_snake_case(&field_ident.to_string()));
        let column_name_lit = LitStr::new(&column_name, Span::call_site());
        let default_sql = match options.default_sql {
            Some(default_sql) => quote!(Some(#default_sql)),
            None => quote!(None),
        };
        if let Some(foreign_key) = options.foreign_key {
            let references_table_lit =
                LitStr::new(&foreign_key.references_table, Span::call_site());
            let references_column_lit =
                LitStr::new(&foreign_key.references_column, Span::call_site());
            foreign_key_defs.push(quote! {
                #comet::nebula::ForeignKeyDef {
                    columns: &[#column_name_lit],
                    references_table: #references_table_lit,
                    references_columns: &[#references_column_lit],
                }
            });
        }
        let nullable = options.nullable;
        let primary_key = options.primary_key;
        let auto_increment = options.auto_increment;
        let unique = options.unique;
        let indexed = options.indexed;
        let field_ty = field.ty;
        let table_name_lit = LitStr::new(&table_name, Span::call_site());

        column_consts.push(quote! {
            pub const #const_ident: #comet::nebula::Column<#field_ty> =
                #comet::nebula::Column::new(#table_name_lit, #column_name_lit);
        });

        column_defs.push(quote! {
            #comet::nebula::ColumnDef {
                name: #column_name_lit,
                sql_type: #sql_type,
                nullable: #nullable,
                primary_key: #primary_key,
                auto_increment: #auto_increment,
                unique: #unique,
                indexed: #indexed,
                default_sql: #default_sql,
            }
        });
    }

    validate_rls_policies(&struct_options.rls, &column_names, &ident)?;
    let rls_defs = struct_options
        .rls
        .iter()
        .map(|policy| rls_policy_tokens(policy, &comet))
        .collect::<Vec<_>>();

    if primary_key_count > 1 {
        return Err(syn::Error::new_spanned(
            ident,
            "Nebula Entity supports at most one primary key in the derive MVP",
        ));
    }

    let table_name_lit = LitStr::new(&table_name, Span::call_site());
    Ok(quote! {
        impl #ident {
            #(#column_consts)*
        }

        impl #comet::nebula::Entity for #ident {
            const TABLE: #comet::nebula::TableDef = #comet::nebula::TableDef {
                name: #table_name_lit,
                columns: &[#(#column_defs),*],
                indexes: &[],
                foreign_keys: &[#(#foreign_key_defs),*],
                rls: &[#(#rls_defs),*],
            };
        }
    })
}

#[derive(Default)]
struct StructOptions {
    table: Option<String>,
    comet_path: Option<Path>,
    rls: Vec<RlsPolicyAttr>,
}

impl StructOptions {
    fn parse(attrs: &[syn::Attribute]) -> Result<Self> {
        let mut options = StructOptions::default();

        for attr in attrs.iter().filter(|attr| attr.path().is_ident("nebula")) {
            attr.parse_args_with(|input: ParseStream<'_>| {
                while !input.is_empty() {
                    let key: Ident = input.parse()?;

                    if key == "rls" {
                        let content;
                        syn::parenthesized!(content in input);
                        options.rls.push(parse_rls_policy(&content)?);
                    } else if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;

                        if key == "table" {
                            options.table = Some(input.parse::<LitStr>()?.value());
                        } else if key == "crate" {
                            options.comet_path = Some(input.parse::<LitStr>()?.parse()?);
                        } else {
                            return Err(syn::Error::new_spanned(
                                key,
                                "unsupported Nebula struct attribute",
                            ));
                        }
                    } else {
                        return Err(syn::Error::new_spanned(
                            key,
                            "unsupported Nebula struct attribute",
                        ));
                    }

                    if input.peek(Token![,]) {
                        input.parse::<Token![,]>()?;
                    }
                }

                Ok(())
            })?;
        }

        Ok(options)
    }
}

#[derive(Debug, Clone, Copy)]
enum RlsOperationAttr {
    Select,
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RlsKindAttr {
    Public,
    Owner,
    Tenant,
    Rbac,
    Custom,
}

#[derive(Debug, Clone, Copy)]
enum RlsModeAttr {
    All,
    Any,
}

#[derive(Debug, Clone)]
struct RlsPolicyAttr {
    operations: Vec<RlsOperationAttr>,
    kind: Option<RlsKindAttr>,
    column: Option<String>,
    mode: RlsModeAttr,
    roles: Vec<String>,
    permissions: Vec<String>,
    scopes: Vec<String>,
    resource: Option<String>,
    custom: Option<String>,
}

impl Default for RlsPolicyAttr {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            kind: None,
            column: None,
            mode: RlsModeAttr::All,
            roles: Vec::new(),
            permissions: Vec::new(),
            scopes: Vec::new(),
            resource: None,
            custom: None,
        }
    }
}

fn parse_rls_policy(input: ParseStream<'_>) -> Result<RlsPolicyAttr> {
    let mut policy = RlsPolicyAttr::default();

    while !input.is_empty() {
        let key: Ident = input.parse()?;
        let key_name = key.to_string();

        if let Some(operation) = parse_rls_operation(&key_name) {
            policy.operations.push(operation);
        } else if key_name == "public" {
            set_rls_kind(&mut policy, RlsKindAttr::Public, &key)?;
        } else if key_name == "any" || key_name == "all" {
            let content;
            syn::parenthesized!(content in input);
            policy.mode = if key_name == "any" {
                RlsModeAttr::Any
            } else {
                RlsModeAttr::All
            };
            parse_rls_requirements(&content, &mut policy)?;
            set_rls_kind(&mut policy, RlsKindAttr::Rbac, &key)?;
        } else if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            match key_name.as_str() {
                "owner" => {
                    policy.column = Some(input.parse::<LitStr>()?.value());
                    set_rls_kind(&mut policy, RlsKindAttr::Owner, &key)?;
                }
                "tenant" => {
                    policy.column = Some(input.parse::<LitStr>()?.value());
                    set_rls_kind(&mut policy, RlsKindAttr::Tenant, &key)?;
                }
                "role" => {
                    policy.roles.push(input.parse::<LitStr>()?.value());
                    set_rls_kind(&mut policy, RlsKindAttr::Rbac, &key)?;
                }
                "permission" => {
                    policy.permissions.push(input.parse::<LitStr>()?.value());
                    set_rls_kind(&mut policy, RlsKindAttr::Rbac, &key)?;
                }
                "scope" => {
                    policy.scopes.push(input.parse::<LitStr>()?.value());
                    set_rls_kind(&mut policy, RlsKindAttr::Rbac, &key)?;
                }
                "resource" => {
                    policy.resource = Some(input.parse::<LitStr>()?.value());
                }
                "custom" => {
                    policy.custom = Some(input.parse::<LitStr>()?.value());
                    set_rls_kind(&mut policy, RlsKindAttr::Custom, &key)?;
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        key,
                        "unsupported Nebula RLS attribute",
                    ));
                }
            }
        } else {
            return Err(syn::Error::new_spanned(
                key,
                "unsupported Nebula RLS attribute",
            ));
        }

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
    }

    if policy.kind.is_none() {
        return Err(input.error("Nebula `rls` requires a policy kind"));
    }

    Ok(policy)
}

fn parse_rls_requirements(input: ParseStream<'_>, policy: &mut RlsPolicyAttr) -> Result<()> {
    while !input.is_empty() {
        let key: Ident = input.parse()?;
        let key_name = key.to_string();
        input.parse::<Token![=]>()?;
        let value = input.parse::<LitStr>()?.value();

        match key_name.as_str() {
            "role" => policy.roles.push(value),
            "permission" => policy.permissions.push(value),
            "scope" => policy.scopes.push(value),
            "resource" => policy.resource = Some(value),
            _ => {
                return Err(syn::Error::new_spanned(
                    key,
                    "unsupported Nebula RLS authorization attribute",
                ));
            }
        }

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
    }

    Ok(())
}

fn parse_rls_operation(value: &str) -> Option<RlsOperationAttr> {
    match value {
        "select" => Some(RlsOperationAttr::Select),
        "insert" => Some(RlsOperationAttr::Insert),
        "update" => Some(RlsOperationAttr::Update),
        "delete" => Some(RlsOperationAttr::Delete),
        _ => None,
    }
}

fn set_rls_kind(policy: &mut RlsPolicyAttr, kind: RlsKindAttr, key: &Ident) -> Result<()> {
    if let Some(current) = policy.kind {
        if current != kind {
            return Err(syn::Error::new_spanned(
                key,
                "Nebula `rls` policy cannot combine multiple policy kinds",
            ));
        }
    }

    policy.kind = Some(kind);
    Ok(())
}

fn validate_rls_policies(
    policies: &[RlsPolicyAttr],
    column_names: &HashSet<String>,
    ident: &Ident,
) -> Result<()> {
    for policy in policies {
        let kind = policy.kind.expect("parsed RLS policy kind");
        if let Some(column) = &policy.column {
            if !column_names.contains(column) {
                return Err(syn::Error::new_spanned(
                    ident,
                    format!("Nebula `rls` references unknown column `{column}`"),
                ));
            }
        }

        match kind {
            RlsKindAttr::Public => {
                if !policy.operations.is_empty()
                    || policy.column.is_some()
                    || !policy.roles.is_empty()
                    || !policy.permissions.is_empty()
                    || !policy.scopes.is_empty()
                    || policy.resource.is_some()
                    || policy.custom.is_some()
                {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula `rls(public)` cannot include operations, columns, authorization, resource, or custom predicates",
                    ));
                }
            }
            RlsKindAttr::Owner | RlsKindAttr::Tenant => {
                if policy.column.is_none() {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula owner/tenant RLS requires a column",
                    ));
                }
                if !policy.roles.is_empty()
                    || !policy.permissions.is_empty()
                    || !policy.scopes.is_empty()
                    || policy.resource.is_some()
                    || policy.custom.is_some()
                {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula owner/tenant RLS cannot include authorization, resource, or custom predicates",
                    ));
                }
            }
            RlsKindAttr::Rbac => {
                if policy.roles.is_empty() && policy.permissions.is_empty() && policy.scopes.is_empty()
                {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula RBAC RLS requires at least one role, permission, or scope",
                    ));
                }
                if policy.column.is_some() || policy.custom.is_some() {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula RBAC RLS cannot include owner/tenant columns or custom predicates",
                    ));
                }
            }
            RlsKindAttr::Custom => {
                if policy.custom.as_deref().is_none_or(str::is_empty) {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula custom RLS requires a non-empty predicate name",
                    ));
                }
                if policy.column.is_some()
                    || !policy.roles.is_empty()
                    || !policy.permissions.is_empty()
                    || !policy.scopes.is_empty()
                    || policy.resource.is_some()
                {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "Nebula custom RLS cannot include owner/tenant columns or authorization",
                    ));
                }
            }
        }
    }

    Ok(())
}

fn rls_policy_tokens(policy: &RlsPolicyAttr, comet: &Path) -> proc_macro2::TokenStream {
    let operations = policy.operations.iter().map(|operation| match operation {
        RlsOperationAttr::Select => quote!(#comet::nebula::RlsOperation::Select),
        RlsOperationAttr::Insert => quote!(#comet::nebula::RlsOperation::Insert),
        RlsOperationAttr::Update => quote!(#comet::nebula::RlsOperation::Update),
        RlsOperationAttr::Delete => quote!(#comet::nebula::RlsOperation::Delete),
    });
    let kind = match policy.kind.expect("validated RLS policy kind") {
        RlsKindAttr::Public => quote!(#comet::nebula::RlsPolicyKind::Public),
        RlsKindAttr::Owner => quote!(#comet::nebula::RlsPolicyKind::Owner),
        RlsKindAttr::Tenant => quote!(#comet::nebula::RlsPolicyKind::Tenant),
        RlsKindAttr::Rbac => quote!(#comet::nebula::RlsPolicyKind::Rbac),
        RlsKindAttr::Custom => quote!(#comet::nebula::RlsPolicyKind::Custom),
    };
    let column = match &policy.column {
        Some(column) => quote!(Some(#column)),
        None => quote!(None),
    };
    let mode = match policy.mode {
        RlsModeAttr::All => quote!(#comet::nebula::RlsMatchMode::All),
        RlsModeAttr::Any => quote!(#comet::nebula::RlsMatchMode::Any),
    };
    let roles = policy.roles.iter();
    let permissions = policy.permissions.iter();
    let scopes = policy.scopes.iter();
    let resource = match &policy.resource {
        Some(resource) => quote!(Some(#resource)),
        None => quote!(None),
    };
    let custom = match &policy.custom {
        Some(custom) => quote!(Some(#custom)),
        None => quote!(None),
    };

    quote! {
        #comet::nebula::RlsPolicyDef {
            operations: &[#(#operations),*],
            kind: #kind,
            column: #column,
            authorization: #comet::nebula::RlsAuthorizationDef {
                mode: #mode,
                roles: &[#(#roles),*],
                permissions: &[#(#permissions),*],
                scopes: &[#(#scopes),*],
                resource: #resource,
            },
            custom: #custom,
        }
    }
}

#[derive(Default)]
struct FieldOptions {
    rename: Option<String>,
    primary_key: bool,
    auto_increment: bool,
    unique: bool,
    indexed: bool,
    nullable: bool,
    default_sql: Option<LitStr>,
    foreign_key: Option<ForeignKeyAttr>,
}

impl FieldOptions {
    fn parse(field: &Field) -> Result<Self> {
        let mut options = FieldOptions::default();

        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("nebula"))
        {
            attr.parse_args_with(|input: ParseStream<'_>| {
                while !input.is_empty() {
                    let item: NebulaFieldItem = input.parse()?;

                    match item {
                        NebulaFieldItem::Flag(flag) if flag == "primary_key" => {
                            options.primary_key = true;
                        }
                        NebulaFieldItem::Flag(flag)
                            if flag == "auto" || flag == "auto_increment" =>
                        {
                            options.auto_increment = true;
                        }
                        NebulaFieldItem::Flag(flag) if flag == "unique" => {
                            options.unique = true;
                        }
                        NebulaFieldItem::Flag(flag) if flag == "index" || flag == "indexed" => {
                            options.indexed = true;
                        }
                        NebulaFieldItem::Flag(flag) if flag == "nullable" => {
                            options.nullable = true;
                        }
                        NebulaFieldItem::NameValue(key, value) if key == "rename" => {
                            options.rename = Some(expect_string(value, "rename")?);
                        }
                        NebulaFieldItem::NameValue(key, value) if key == "default" => {
                            options.default_sql = Some(expect_lit_string(value, "default")?);
                        }
                        NebulaFieldItem::NameValue(key, value) if key == "foreign_key" => {
                            options.foreign_key =
                                Some(parse_foreign_key(expect_lit_string(value, "foreign_key")?)?);
                        }
                        NebulaFieldItem::NameValue(key, value) if key == "nullable" => {
                            options.nullable = expect_bool(value, "nullable")?;
                        }
                        NebulaFieldItem::Flag(flag) | NebulaFieldItem::NameValue(flag, _) => {
                            return Err(syn::Error::new_spanned(
                                flag,
                                "unsupported Nebula field attribute",
                            ));
                        }
                    }

                    if input.peek(Token![,]) {
                        input.parse::<Token![,]>()?;
                    }
                }

                Ok(())
            })?;
        }

        Ok(options)
    }
}

struct ForeignKeyAttr {
    references_table: String,
    references_column: String,
}

enum NebulaFieldItem {
    Flag(Ident),
    NameValue(Ident, Expr),
}

impl Parse for NebulaFieldItem {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let key: Ident = input.parse()?;

        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            Ok(NebulaFieldItem::NameValue(key, input.parse()?))
        } else {
            Ok(NebulaFieldItem::Flag(key))
        }
    }
}

fn expect_lit_string(value: Expr, name: &str) -> Result<LitStr> {
    match value {
        Expr::Lit(lit) => match lit.lit {
            syn::Lit::Str(value) => Ok(value),
            other => Err(syn::Error::new_spanned(
                other,
                format!("Nebula `{name}` must be a string literal"),
            )),
        },
        other => Err(syn::Error::new_spanned(
            other,
            format!("Nebula `{name}` must be a string literal"),
        )),
    }
}

fn expect_string(value: Expr, name: &str) -> Result<String> {
    Ok(expect_lit_string(value, name)?.value())
}

fn expect_bool(value: Expr, name: &str) -> Result<bool> {
    match value {
        Expr::Lit(lit) => match lit.lit {
            syn::Lit::Bool(LitBool { value, .. }) => Ok(value),
            other => Err(syn::Error::new_spanned(
                other,
                format!("Nebula `{name}` must be a bool literal"),
            )),
        },
        other => Err(syn::Error::new_spanned(
            other,
            format!("Nebula `{name}` must be a bool literal"),
        )),
    }
}

fn parse_foreign_key(value: LitStr) -> Result<ForeignKeyAttr> {
    let raw = value.value();
    let Some((table, column)) = raw.split_once('.') else {
        return Err(syn::Error::new_spanned(
            value,
            "Nebula `foreign_key` must use `table.column` syntax",
        ));
    };

    if table.is_empty() || column.is_empty() || column.contains('.') {
        return Err(syn::Error::new_spanned(
            value,
            "Nebula `foreign_key` must use `table.column` syntax",
        ));
    }

    Ok(ForeignKeyAttr {
        references_table: table.to_owned(),
        references_column: column.to_owned(),
    })
}

fn sql_type_tokens(ty: &Type, comet: &Path) -> Result<proc_macro2::TokenStream> {
    let Some(ident) = type_ident(ty) else {
        return Err(syn::Error::new_spanned(ty, "unsupported Nebula field type"));
    };

    let sql_type = match ident.to_string().as_str() {
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => {
            quote!(#comet::nebula::SqlType::Integer)
        }
        "f32" | "f64" => quote!(#comet::nebula::SqlType::Real),
        "String" | "str" => quote!(#comet::nebula::SqlType::Text),
        "Vec" => quote!(#comet::nebula::SqlType::Blob),
        "bool" => quote!(#comet::nebula::SqlType::Boolean),
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!("unsupported Nebula field type `{ident}`"),
            ));
        }
    };

    Ok(sql_type)
}

fn is_integer_type(ty: &Type) -> bool {
    type_ident(ty).is_some_and(|ident| {
        matches!(
            ident.to_string().as_str(),
            "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize"
        )
    })
}

fn type_ident(ty: &Type) -> Option<&Ident> {
    match ty {
        Type::Path(path) => path.path.segments.last().map(|segment| &segment.ident),
        Type::Reference(reference) => type_ident(&reference.elem),
        _ => None,
    }
}

fn to_snake_case(value: &str) -> String {
    let mut output = String::new();

    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }

    output
}

fn to_upper_snake_case(value: &str) -> String {
    let mut output = String::new();

    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character);
        } else if character == '-' {
            output.push('_');
        } else {
            output.push(character.to_ascii_uppercase());
        }
    }

    output
}
