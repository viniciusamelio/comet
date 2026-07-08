use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{FnArg, Item, ItemFn, LitStr, Pat, ReturnType, Token, Type};
use toml::Value;

const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RpcManifest {
    pub version: u32,
    pub routes: Vec<RpcRoute>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RpcRoute {
    pub name: String,
    pub module_path: Vec<String>,
    pub source: String,
    pub method: String,
    pub path: String,
    pub data_param: Option<String>,
    pub path_params: Vec<RpcParam>,
    pub body: Option<String>,
    pub response: Option<String>,
    pub error: Option<String>,
    pub auth: RpcAuth,
    pub support: RpcRouteSupport,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RpcParam {
    pub name: String,
    pub rust_type: String,
    pub variadic: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RpcAuth {
    None,
    Optional,
    Required,
    Authorized {
        policy: Option<String>,
        roles: Vec<String>,
        permissions: Vec<String>,
        scopes: Vec<String>,
        resource: Option<String>,
        mode: AuthMode,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    All,
    Any,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RpcRouteSupport {
    Json,
    Raw,
    Unsupported,
}

#[derive(Debug)]
struct RocketRouteAttr {
    method: String,
    path: String,
    data_param: Option<String>,
}

struct RocketRouteArgs {
    path: LitStr,
    data_param: Option<String>,
}

impl Parse for RocketRouteArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let path = input.parse::<LitStr>()?;
        let mut data_param = None;

        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }

            let key = input.parse::<syn::Ident>()?;
            input.parse::<Token![=]>()?;
            let value = input.parse::<LitStr>()?;
            if key == "data" {
                data_param = Some(strip_angle_binding(&value.value()).to_owned());
            }
        }

        Ok(Self { path, data_param })
    }
}

pub fn discover_manifest(project_dir: &Path) -> Result<RpcManifest> {
    let src_dir = project_dir.join("src");
    let mut routes = Vec::new();
    visit_dir(&src_dir, &[], &mut routes)?;
    resolve_authorization_policies(project_dir, &mut routes)?;
    routes.sort_by(|a, b| {
        a.module_path
            .cmp(&b.module_path)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.method.cmp(&b.method))
    });

    Ok(RpcManifest {
        version: MANIFEST_VERSION,
        routes,
    })
}

fn resolve_authorization_policies(project_dir: &Path, routes: &mut [RpcRoute]) -> Result<()> {
    let pending = routes
        .iter()
        .filter_map(|route| {
            let RpcAuth::Authorized {
                policy: Some(policy),
                roles,
                permissions,
                scopes,
                ..
            } = &route.auth
            else {
                return None;
            };

            if !roles.is_empty() || !permissions.is_empty() || !scopes.is_empty() {
                return None;
            }

            Some(PolicyRef {
                display: policy.clone(),
                path: String::new(),
                module_path: route.module_path.clone(),
            })
        })
        .collect::<Vec<_>>();

    if pending.is_empty() {
        return Ok(());
    }

    if !project_dir.join("Cargo.toml").exists() {
        for route in routes {
            if let RpcAuth::Authorized {
                policy: Some(policy),
                roles,
                permissions,
                scopes,
                ..
            } = &route.auth
                && roles.is_empty()
                && permissions.is_empty()
                && scopes.is_empty()
            {
                route.warnings.push(format!(
                    "authorization policy `{policy}` was not resolved because the project has no Cargo.toml"
                ));
            }
        }
        return Ok(());
    }

    let project = RustProject::read(project_dir)?;
    let policy_refs = pending
        .into_iter()
        .map(|policy_ref| PolicyRef {
            path: policy_path(
                &policy_ref.display,
                &policy_ref.module_path,
                &project.crate_name,
            ),
            ..policy_ref
        })
        .collect::<Vec<_>>();
    let requirements = dump_authorization_requirements(&project, &policy_refs)?;

    for route in routes {
        let RpcAuth::Authorized {
            policy: Some(policy),
            roles,
            permissions,
            scopes,
            resource,
            mode,
        } = &mut route.auth
        else {
            continue;
        };

        if !roles.is_empty() || !permissions.is_empty() || !scopes.is_empty() {
            continue;
        }

        let path = policy_path(policy, &route.module_path, &project.crate_name);
        if let Some(requirement) = requirements.iter().find(|entry| entry.path == path) {
            *roles = requirement.roles.clone();
            *permissions = requirement.permissions.clone();
            *scopes = requirement.scopes.clone();
            *resource = requirement.resource.clone();
            *mode = requirement.mode.clone();
        } else {
            route.warnings.push(format!(
                "authorization policy `{policy}` could not be resolved"
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PolicyRef {
    display: String,
    path: String,
    module_path: Vec<String>,
}

#[derive(Debug)]
struct RustProject {
    dir: PathBuf,
    package_name: String,
    crate_name: String,
    comet_auth_dependency: Value,
}

#[derive(Debug, Deserialize)]
struct AuthorizationRequirementDump {
    path: String,
    mode: AuthMode,
    roles: Vec<String>,
    permissions: Vec<String>,
    scopes: Vec<String>,
    resource: Option<String>,
}

impl<'de> Deserialize<'de> for AuthMode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "all" => Ok(Self::All),
            "any" => Ok(Self::Any),
            other => Err(serde::de::Error::custom(format!(
                "unsupported authorization mode `{other}`"
            ))),
        }
    }
}

impl RustProject {
    fn read(project_dir: &Path) -> Result<Self> {
        let dir = project_dir
            .canonicalize()
            .with_context(|| format!("resolving {}", project_dir.display()))?;
        let cargo_toml_path = dir.join("Cargo.toml");
        let cargo_toml_text = fs::read_to_string(&cargo_toml_path)
            .with_context(|| format!("reading {}", cargo_toml_path.display()))?;
        let root: Value = toml::from_str(&cargo_toml_text)
            .with_context(|| format!("parsing {}", cargo_toml_path.display()))?;

        let package_name = root
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .with_context(|| format!("{} has no [package].name", cargo_toml_path.display()))?
            .to_owned();
        let crate_name = package_name.replace('-', "_");

        let comet_auth_dependency = root
            .get("dependencies")
            .and_then(|deps| deps.get("comet-auth"))
            .with_context(|| {
                format!(
                    "{} has no [dependencies].comet-auth entry",
                    cargo_toml_path.display()
                )
            })?
            .clone();
        let mut comet_auth_dependency = dependency_as_table(&comet_auth_dependency)?;
        resolve_relative_path(&mut comet_auth_dependency, &dir)?;

        Ok(Self {
            dir,
            package_name,
            crate_name,
            comet_auth_dependency,
        })
    }
}

fn dump_authorization_requirements(
    project: &RustProject,
    policy_refs: &[PolicyRef],
) -> Result<Vec<AuthorizationRequirementDump>> {
    if policy_refs.is_empty() {
        return Ok(Vec::new());
    }

    let mut policy_refs = policy_refs.to_vec();
    policy_refs.sort();
    policy_refs.dedup_by(|a, b| a.path == b.path);

    let temp_dir = tempfile::tempdir().context("creating rpc-dump temp directory")?;
    fs::write(
        temp_dir.path().join("Cargo.toml"),
        build_rpc_dump_manifest(project)?,
    )
    .context("writing rpc-dump Cargo.toml")?;
    fs::create_dir_all(temp_dir.path().join("src")).context("creating rpc-dump src/")?;
    fs::write(
        temp_dir.path().join("src/main.rs"),
        render_auth_dump_main_rs(&policy_refs),
    )
    .context("writing rpc-dump main.rs")?;

    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(temp_dir.path())
        .output()
        .context("running cargo for the rpc-dump crate")?;

    if !output.status.success() {
        bail!(
            "RPC authorization policy dump failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let json =
        String::from_utf8(output.stdout).context("RPC authorization dump produced non-UTF8")?;
    serde_json::from_str(&json).context("parsing RPC authorization dump JSON")
}

fn build_rpc_dump_manifest(project: &RustProject) -> Result<String> {
    let mut target_dependency = toml::Table::new();
    target_dependency.insert(
        "path".to_owned(),
        Value::String(project.dir.display().to_string()),
    );

    let mut dependencies = toml::Table::new();
    dependencies.insert(
        project.package_name.clone(),
        Value::Table(target_dependency),
    );
    dependencies.insert(
        "comet-auth".to_owned(),
        project.comet_auth_dependency.clone(),
    );
    dependencies.insert("serde_json".to_owned(), Value::String("1".to_owned()));

    let mut package = toml::Table::new();
    package.insert(
        "name".to_owned(),
        Value::String("comet-rpc-dump".to_owned()),
    );
    package.insert("version".to_owned(), Value::String("0.0.0".to_owned()));
    package.insert("edition".to_owned(), Value::String("2021".to_owned()));
    package.insert("publish".to_owned(), Value::Boolean(false));

    let mut root = toml::Table::new();
    root.insert("package".to_owned(), Value::Table(package));
    root.insert("dependencies".to_owned(), Value::Table(dependencies));

    toml::to_string_pretty(&Value::Table(root)).context("rendering rpc-dump Cargo.toml")
}

fn render_auth_dump_main_rs(policy_refs: &[PolicyRef]) -> String {
    let entries = policy_refs
        .iter()
        .map(|policy_ref| {
            let path = &policy_ref.path;
            format!(
                "    {{\n\
                 \x20       let requirement = <{path} as ::comet_auth::RequiredAuthorization>::REQUIREMENT;\n\
                 \x20       values.push(::serde_json::json!({{\n\
                 \x20           \"path\": \"{path}\",\n\
                 \x20           \"mode\": match requirement.mode {{\n\
                 \x20               ::comet_auth::AuthorizationMode::All => \"all\",\n\
                 \x20               ::comet_auth::AuthorizationMode::Any => \"any\",\n\
                 \x20           }},\n\
                 \x20           \"roles\": requirement.roles,\n\
                 \x20           \"permissions\": requirement.permissions,\n\
                 \x20           \"scopes\": requirement.scopes,\n\
                 \x20           \"resource\": requirement.resource,\n\
                 \x20       }}));\n\
                 \x20   }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "fn main() {{\n\
         \x20   let mut values = Vec::new();\n\
         {entries}\n\
         \x20   print!(\"{{}}\", ::serde_json::to_string(&values).unwrap());\n\
         }}\n"
    )
}

fn dependency_as_table(dependency: &Value) -> Result<Value> {
    match dependency {
        Value::String(version) => {
            let mut table = toml::Table::new();
            table.insert("version".to_owned(), Value::String(version.clone()));
            Ok(Value::Table(table))
        }
        Value::Table(table) => Ok(Value::Table(table.clone())),
        other => bail!("unsupported dependency format in Cargo.toml: {other:?}"),
    }
}

fn resolve_relative_path(dependency: &mut Value, base_dir: &Path) -> Result<()> {
    let Value::Table(table) = dependency else {
        return Ok(());
    };
    let Some(Value::String(path_str)) = table.get("path").cloned() else {
        return Ok(());
    };

    let path = Path::new(&path_str);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    let absolute = absolute
        .canonicalize()
        .with_context(|| format!("resolving path dependency {}", absolute.display()))?;

    table.insert(
        "path".to_owned(),
        Value::String(absolute.display().to_string()),
    );
    Ok(())
}

fn policy_path(policy: &str, module_path: &[String], crate_name: &str) -> String {
    if let Some(rest) = policy.strip_prefix("crate ::") {
        return format!("{crate_name}::{rest}").replace(" :: ", "::");
    }
    if let Some(rest) = policy.strip_prefix("crate::") {
        return format!("{crate_name}::{rest}");
    }
    if policy.starts_with("::") {
        return policy.trim_start_matches("::").replace(" :: ", "::");
    }
    if policy.contains("::") || policy.contains(" :: ") {
        return policy.replace(" :: ", "::");
    }

    let mut segments = vec![crate_name.to_owned()];
    segments.extend(module_path.iter().cloned());
    segments.push(policy.to_owned());
    segments.join("::")
}

fn visit_dir(dir: &Path, module_path: &[String], routes: &mut Vec<RpcRoute>) -> Result<()> {
    let mut dir_entries = fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading directory {}", dir.display()))?;
    dir_entries.sort_by_key(|entry| entry.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type of {}", path.display()))?;

        if file_type.is_dir() {
            let mut nested_path = module_path.to_vec();
            nested_path.push(entry.file_name().to_string_lossy().into_owned());
            visit_dir(&path, &nested_path, routes)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            visit_file(&path, module_path, routes)?;
        }
    }

    Ok(())
}

fn visit_file(path: &Path, module_path: &[String], routes: &mut Vec<RpcRoute>) -> Result<()> {
    let file_module_path = file_module_path(path, module_path);
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file = syn::parse_file(&source).with_context(|| format!("parsing {}", path.display()))?;

    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            for route_attr in rocket_route_attrs(item_fn) {
                routes.push(route_from_fn(path, &file_module_path, item_fn, route_attr));
            }
        }
    }

    Ok(())
}

fn file_module_path(path: &Path, module_path: &[String]) -> Vec<String> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();

    match stem {
        "mod" | "lib" | "main" => module_path.to_vec(),
        _ => {
            let mut nested = module_path.to_vec();
            nested.push(stem.to_owned());
            nested
        }
    }
}

fn rocket_route_attrs(item_fn: &ItemFn) -> Vec<RocketRouteAttr> {
    item_fn
        .attrs
        .iter()
        .filter_map(|attr| {
            let method = route_method(attr.path().segments.last()?.ident.to_string().as_str())?;
            let args = attr.parse_args::<RocketRouteArgs>().ok()?;
            Some(RocketRouteAttr {
                method: method.to_owned(),
                path: args.path.value(),
                data_param: args.data_param,
            })
        })
        .collect()
}

fn route_method(ident: &str) -> Option<&'static str> {
    match ident {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "delete" => Some("DELETE"),
        "patch" => Some("PATCH"),
        _ => None,
    }
}

fn route_from_fn(
    path: &Path,
    module_path: &[String],
    item_fn: &ItemFn,
    route_attr: RocketRouteAttr,
) -> RpcRoute {
    let mut warnings = Vec::new();
    let inputs = route_inputs(item_fn);
    let path_params = discover_path_params(&route_attr.path, &inputs, &mut warnings);
    let body = route_attr
        .data_param
        .as_deref()
        .and_then(|name| inputs.iter().find(|input| input.name == name))
        .and_then(|input| json_inner_type(&input.rust_type));
    let (response, error) = response_and_error(item_fn);
    let auth = auth_for_route(item_fn, &inputs);
    let support = classify_support(
        &inputs,
        response.as_deref(),
        error.as_deref(),
        body.as_deref(),
    );

    if route_attr.data_param.is_some() && body.is_none() {
        warnings.push("data parameter is not a supported Json<T> body".to_owned());
    }

    if response.is_none() && support == RpcRouteSupport::Json {
        warnings.push("response type could not be inferred".to_owned());
    }

    RpcRoute {
        name: item_fn.sig.ident.to_string(),
        module_path: module_path.to_vec(),
        source: path.display().to_string(),
        method: route_attr.method,
        path: route_attr.path,
        data_param: route_attr.data_param,
        path_params,
        body,
        response,
        error,
        auth,
        support,
        warnings,
    }
}

#[derive(Debug)]
struct RouteInput {
    name: String,
    rust_type: String,
}

fn route_inputs(item_fn: &ItemFn) -> Vec<RouteInput> {
    item_fn
        .sig
        .inputs
        .iter()
        .filter_map(|input| {
            let FnArg::Typed(pat_type) = input else {
                return None;
            };
            let Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
                return None;
            };

            Some(RouteInput {
                name: pat_ident.ident.to_string(),
                rust_type: type_to_string(&pat_type.ty),
            })
        })
        .collect()
}

fn discover_path_params(
    path: &str,
    inputs: &[RouteInput],
    warnings: &mut Vec<String>,
) -> Vec<RpcParam> {
    let mut params = Vec::new();
    for segment in path.split('/') {
        let Some(raw) = segment.strip_prefix('<').and_then(|s| s.strip_suffix('>')) else {
            continue;
        };

        let (name, variadic) = match raw.strip_suffix("..") {
            Some(name) => (name, true),
            None => (raw, false),
        };

        match inputs.iter().find(|input| input.name == name) {
            Some(input) => params.push(RpcParam {
                name: name.to_owned(),
                rust_type: input.rust_type.clone(),
                variadic,
            }),
            None => warnings.push(format!("path parameter `{name}` has no matching argument")),
        }
    }
    params
}

fn response_and_error(item_fn: &ItemFn) -> (Option<String>, Option<String>) {
    let ReturnType::Type(_, ty) = &item_fn.sig.output else {
        return (None, None);
    };

    let output = type_to_string(ty);
    if let Some(inner) = json_inner_type(&output) {
        return (Some(inner), None);
    }

    if let Some((ok, err)) =
        split_result_like(&output, "Result").or_else(|| split_result_like(&output, "ApiResult"))
    {
        return (json_inner_type(&ok).or(Some(ok)), err);
    }

    (None, Some(output))
}

fn split_result_like(output: &str, wrapper: &str) -> Option<(String, Option<String>)> {
    let generic = generic_inner_for_last_segment(output, wrapper)?;
    let parts = split_top_level_commas(&generic);
    match parts.as_slice() {
        [ok] if wrapper == "ApiResult" => Some((ok.to_string(), Some(wrapper.to_owned()))),
        [ok, err] => Some((ok.to_string(), Some(err.to_string()))),
        _ => None,
    }
}

fn auth_for_route(item_fn: &ItemFn, inputs: &[RouteInput]) -> RpcAuth {
    if let Some(auth) = auth_from_requires_auth_attr(item_fn) {
        return auth;
    }

    for input in inputs {
        if type_last_segment(&input.rust_type) == Some("OptionalAuthSession") {
            return RpcAuth::Optional;
        }
    }

    for input in inputs {
        match type_last_segment(&input.rust_type) {
            Some("AuthSession") => return RpcAuth::Required,
            Some("AuthorizedSession") => {
                return RpcAuth::Authorized {
                    policy: generic_inner_for_last_segment(&input.rust_type, "AuthorizedSession"),
                    roles: Vec::new(),
                    permissions: Vec::new(),
                    scopes: Vec::new(),
                    resource: None,
                    mode: AuthMode::All,
                };
            }
            _ => {}
        }
    }

    RpcAuth::None
}

fn auth_from_requires_auth_attr(item_fn: &ItemFn) -> Option<RpcAuth> {
    let attr = item_fn.attrs.iter().find(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "requires_auth")
    })?;

    let args = attr
        .parse_args_with(Punctuated::<RequiresAuthArg, Token![,]>::parse_terminated)
        .ok()?;
    let mut auth = ParsedRequiresAuth::default();
    for arg in args {
        match arg {
            RequiresAuthArg::Optional => auth.optional = true,
            RequiresAuthArg::Resource(resource) => auth.resource = Some(resource),
            RequiresAuthArg::Claim(claim) => auth.push(claim),
            RequiresAuthArg::Group { mode, claims } => {
                auth.mode = mode;
                for claim in claims {
                    auth.push(claim);
                }
            }
        }
    }

    if auth.optional {
        Some(RpcAuth::Optional)
    } else if auth.has_policy() {
        Some(RpcAuth::Authorized {
            policy: None,
            roles: auth.roles,
            permissions: auth.permissions,
            scopes: auth.scopes,
            resource: auth.resource,
            mode: auth.mode,
        })
    } else {
        Some(RpcAuth::Required)
    }
}

#[derive(Default)]
struct ParsedRequiresAuth {
    optional: bool,
    roles: Vec<String>,
    permissions: Vec<String>,
    scopes: Vec<String>,
    resource: Option<String>,
    mode: AuthMode,
}

impl ParsedRequiresAuth {
    fn push(&mut self, claim: RequiresAuthClaim) {
        match claim {
            RequiresAuthClaim::Role(value) => self.roles.push(value),
            RequiresAuthClaim::Permission(value) => self.permissions.push(value),
            RequiresAuthClaim::Scope(value) => self.scopes.push(value),
        }
    }

    fn has_policy(&self) -> bool {
        !self.roles.is_empty() || !self.permissions.is_empty() || !self.scopes.is_empty()
    }
}

impl Default for AuthMode {
    fn default() -> Self {
        Self::All
    }
}

enum RequiresAuthArg {
    Optional,
    Resource(String),
    Claim(RequiresAuthClaim),
    Group {
        mode: AuthMode,
        claims: Vec<RequiresAuthClaim>,
    },
}

enum RequiresAuthClaim {
    Role(String),
    Permission(String),
    Scope(String),
}

impl Parse for RequiresAuthArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse::<syn::Ident>()?;
        match name.to_string().as_str() {
            "optional" => Ok(Self::Optional),
            "resource" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Resource(input.parse::<LitStr>()?.value()))
            }
            "role" | "permission" | "scope" => {
                input.parse::<Token![=]>()?;
                claim_from_name(&name.to_string(), input.parse::<LitStr>()?.value())
                    .map(Self::Claim)
            }
            "any" | "all" => {
                let mode = if name == "any" {
                    AuthMode::Any
                } else {
                    AuthMode::All
                };
                let content;
                syn::parenthesized!(content in input);
                let parsed =
                    Punctuated::<RequiresAuthClaim, Token![,]>::parse_terminated(&content)?;
                Ok(Self::Group {
                    mode,
                    claims: parsed.into_iter().collect(),
                })
            }
            _ => Err(syn::Error::new_spanned(
                name,
                "unsupported requires_auth argument",
            )),
        }
    }
}

impl Parse for RequiresAuthClaim {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse::<syn::Ident>()?;
        input.parse::<Token![=]>()?;
        claim_from_name(&name.to_string(), input.parse::<LitStr>()?.value())
    }
}

fn claim_from_name(name: &str, value: String) -> syn::Result<RequiresAuthClaim> {
    match name {
        "role" => Ok(RequiresAuthClaim::Role(value)),
        "permission" => Ok(RequiresAuthClaim::Permission(value)),
        "scope" => Ok(RequiresAuthClaim::Scope(value)),
        _ => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "unsupported requires_auth claim",
        )),
    }
}

fn classify_support(
    inputs: &[RouteInput],
    response: Option<&str>,
    error_or_responder: Option<&str>,
    body: Option<&str>,
) -> RpcRouteSupport {
    if inputs
        .iter()
        .any(|input| is_raw_or_stream_type(&input.rust_type))
        || response.is_some_and(is_raw_or_stream_type)
        || error_or_responder.is_some_and(is_raw_or_stream_type)
    {
        return RpcRouteSupport::Raw;
    }

    if body.is_some() || response.is_some() {
        return RpcRouteSupport::Json;
    }

    RpcRouteSupport::Unsupported
}

fn is_raw_or_stream_type(rust_type: &str) -> bool {
    [
        "ByteStream",
        "WebSocketResponse",
        "WebSocketUpgrade",
        "R2Object",
        "Capped",
        "Status",
        "String",
        "Vec < u8 >",
        "Vec<u8>",
    ]
    .iter()
    .any(|needle| rust_type.contains(needle))
}

fn json_inner_type(rust_type: &str) -> Option<String> {
    generic_inner_for_last_segment(rust_type, "Json")
}

fn generic_inner_for_last_segment(rust_type: &str, segment: &str) -> Option<String> {
    let patterns = [
        format!("{segment} <"),
        format!("{segment}<"),
        format!(":: {segment} <"),
        format!("::{segment}<"),
    ];
    let start = patterns
        .iter()
        .filter_map(|pattern| rust_type.find(pattern).map(|index| index + pattern.len()))
        .next()?;
    let end = matching_generic_end(rust_type, start)?;
    Some(rust_type[start..end].trim().to_owned())
}

fn matching_generic_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (offset, ch) in text[start..].char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_commas(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, ch) in text.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(text[start..index].trim().to_owned());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(text[start..].trim().to_owned());
    parts
}

fn type_last_segment(rust_type: &str) -> Option<&str> {
    let head = rust_type.split('<').next().unwrap_or(rust_type).trim();
    head.split("::").last().map(str::trim)
}

fn strip_angle_binding(value: &str) -> &str {
    value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(value)
}

fn type_to_string(ty: &Type) -> String {
    ty.to_token_stream().to_string()
}

#[allow(dead_code)]
fn _normalize_path(path: PathBuf) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, relative: &str, contents: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn discovers_json_routes_and_auth_guards() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "src/tasks/routes.rs",
            r#"
            use comet::cloudflare::D1;
            use comet_auth::{AuthSession, AuthorizedSession};
            use rocket::serde::json::Json;

            pub struct DB;
            pub struct TaskWritePolicy;
            pub struct Task;
            pub struct NewTask;
            pub struct ApiError;
            pub type ApiResult<T> = Result<T, ApiError>;

            #[get("/tasks")]
            pub async fn list_tasks(session: AuthSession, db: D1<DB>) -> ApiResult<Json<Vec<Task>>> {
                todo!()
            }

            #[post("/tasks", data = "<new_task>")]
            pub async fn create_task(new_task: Json<NewTask>, session: AuthSession) -> ApiResult<Json<Task>> {
                todo!()
            }

            #[post("/tasks/<id>/complete")]
            pub async fn complete_task(id: i32, session: AuthorizedSession<TaskWritePolicy>) -> ApiResult<Json<Task>> {
                todo!()
            }
            "#,
        );

        let manifest = discover_manifest(dir.path()).unwrap();

        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert_eq!(manifest.routes.len(), 3);
        assert_eq!(manifest.routes[0].name, "complete_task");
        assert_eq!(manifest.routes[0].path_params[0].name, "id");
        assert_eq!(manifest.routes[0].path_params[0].rust_type, "i32");
        assert_eq!(
            manifest.routes[0].auth,
            RpcAuth::Authorized {
                policy: Some("TaskWritePolicy".to_owned()),
                roles: Vec::new(),
                permissions: Vec::new(),
                scopes: Vec::new(),
                resource: None,
                mode: AuthMode::All,
            }
        );
        assert_eq!(manifest.routes[1].body, Some("NewTask".to_owned()));
        assert_eq!(manifest.routes[1].response, Some("Task".to_owned()));
        assert_eq!(manifest.routes[2].response, Some("Vec < Task >".to_owned()));
        assert_eq!(manifest.routes[2].auth, RpcAuth::Required);
    }

    #[test]
    fn parses_requires_auth_macro_metadata() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "src/demo/routes.rs",
            r#"
            #[comet_auth::requires_auth(any(role = "admin", permission = "tasks:review"), resource = "demo")]
            #[get("/private/reviewer")]
            pub async fn private_reviewer() -> &'static str {
                "reviewer"
            }
            "#,
        );

        let manifest = discover_manifest(dir.path()).unwrap();

        assert_eq!(
            manifest.routes[0].auth,
            RpcAuth::Authorized {
                policy: None,
                roles: vec!["admin".to_owned()],
                permissions: vec!["tasks:review".to_owned()],
                scopes: Vec::new(),
                resource: Some("demo".to_owned()),
                mode: AuthMode::Any,
            }
        );
        assert_eq!(manifest.routes[0].support, RpcRouteSupport::Unsupported);
    }

    #[test]
    fn marks_raw_routes() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "src/assets/routes.rs",
            r#"
            use std::path::PathBuf;
            use rocket::data::Capped;
            use rocket::http::Status;

            #[put("/assets/<key..>", data = "<body>")]
            pub async fn put_asset(key: PathBuf, body: Capped<Vec<u8>>) -> Result<Status, Status> {
                todo!()
            }
            "#,
        );

        let manifest = discover_manifest(dir.path()).unwrap();

        assert_eq!(manifest.routes[0].support, RpcRouteSupport::Raw);
        assert_eq!(manifest.routes[0].path_params[0].name, "key");
        assert!(manifest.routes[0].path_params[0].variadic);
    }

    #[test]
    fn resolves_required_authorization_policy_from_target_crate() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let comet_auth_path = workspace.join("comet-auth");
        let rocket_path = workspace.join("vendor/rocket/core/lib");

        write(
            dir.path(),
            "Cargo.toml",
            &format!(
                r#"
                [package]
                name = "rpc-policy-fixture"
                version = "0.0.0"
                edition = "2021"

                [dependencies]
                comet-auth = {{ path = "{}", default-features = false }}
                rocket = {{ path = "{}", default-features = false, features = ["json"] }}
                "#,
                comet_auth_path.display(),
                rocket_path.display(),
            ),
        );
        write(dir.path(), "src/lib.rs", "pub mod routes;\n");
        write(
            dir.path(),
            "src/routes.rs",
            r#"
            use comet_auth::{
                AuthorizationMode, AuthorizationRequirement, AuthorizedSession,
                RequiredAuthorization,
            };

            pub struct WritePolicy;

            impl RequiredAuthorization for WritePolicy {
                const REQUIREMENT: AuthorizationRequirement =
                    AuthorizationRequirement::with_mode_and_resource(
                        AuthorizationMode::Any,
                        &["admin"],
                        &["tasks:write"],
                        &["tasks:review"],
                        Some("tasks"),
                    );
            }

            #[rocket::post("/write")]
            pub async fn write(_session: AuthorizedSession<WritePolicy>) -> &'static str {
                "ok"
            }
            "#,
        );

        let manifest = discover_manifest(dir.path()).unwrap();
        let RpcAuth::Authorized {
            roles,
            permissions,
            scopes,
            resource,
            mode,
            ..
        } = &manifest.routes[0].auth
        else {
            panic!("expected authorized route");
        };

        assert_eq!(roles, &vec!["admin".to_owned()]);
        assert_eq!(permissions, &vec!["tasks:write".to_owned()]);
        assert_eq!(scopes, &vec!["tasks:review".to_owned()]);
        assert_eq!(resource, &Some("tasks".to_owned()));
        assert_eq!(mode, &AuthMode::Any);
    }
}
