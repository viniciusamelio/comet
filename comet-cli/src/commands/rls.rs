use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use comet::nebula::schema::{OwnedRlsPolicyDef, SchemaSnapshot};
use comet::nebula::{RlsOperation, RlsPolicyKind, SchemaLint};
use serde_json::json;

use crate::cli::RlsStatusArgs;
use crate::{discover, schema_dump};

pub fn status(args: RlsStatusArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let snapshot = dump_current_schema(&project_dir)?;
    let custom_registry =
        CustomRegistry::parse(&args.custom_predicates, &args.custom_predicate_rules)?;
    let mut failures = 0usize;
    let mut table_reports = Vec::new();

    for table in &snapshot.tables {
        let missing_operations = missing_operations(&table.rls);
        let missing_custom_predicates = missing_custom_predicates(&table.rls, &custom_registry);
        let table_failures =
            table_failures(&table.rls) + missing_operations.len() + missing_custom_predicates.len();
        failures += table_failures;
        table_reports.push((
            table.name.clone(),
            table.rls.clone(),
            missing_operations,
            missing_custom_predicates,
            table_failures,
        ));
    }

    let schema_lints = snapshot.clone().to_manifest().lint();
    failures += schema_lints.len();

    if args.json {
        print_json_report(&table_reports, &schema_lints, failures)?;
    } else {
        print_text_report(&table_reports, &schema_lints);
    }

    if args.strict && failures > 0 {
        bail!("RLS coverage has {failures} failure(s)");
    }

    Ok(())
}

fn print_text_report(
    table_reports: &[(
        String,
        Vec<OwnedRlsPolicyDef>,
        Vec<RlsOperation>,
        Vec<String>,
        usize,
    )],
    schema_lints: &[SchemaLint],
) {
    println!("RLS coverage ({} table(s)):", table_reports.len());
    for (table, policies, missing_operations, missing_custom_predicates, table_failures) in
        table_reports
    {
        if policies.is_empty() {
            println!("  - {table}: missing");
        } else {
            let status = if *table_failures == 0 {
                "covered"
            } else {
                "incomplete"
            };
            println!("  - {table}: {status}");
            for policy in policies {
                println!("      {}", describe_policy(policy));
            }
        }

        if !missing_operations.is_empty() {
            println!(
                "      missing operations: {}",
                missing_operations
                    .iter()
                    .map(operation_name)
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        if !missing_custom_predicates.is_empty() {
            println!(
                "      missing custom predicates: {}",
                missing_custom_predicates.join(",")
            );
        }
    }

    if !schema_lints.is_empty() {
        println!();
        println!("RLS schema lints:");
        for lint in schema_lints {
            println!("  - {}", describe_schema_lint(lint));
        }
    }
}

fn print_json_report(
    table_reports: &[(
        String,
        Vec<OwnedRlsPolicyDef>,
        Vec<RlsOperation>,
        Vec<String>,
        usize,
    )],
    schema_lints: &[SchemaLint],
    failures: usize,
) -> Result<()> {
    let tables = table_reports
        .iter()
        .map(|(table, policies, missing_operations, missing_custom_predicates, table_failures)| {
            json!({
                "table": table,
                "status": if policies.is_empty() {
                    "missing"
                } else if *table_failures == 0 {
                    "covered"
                } else {
                    "incomplete"
                },
                "missing_operations": missing_operations.iter().map(operation_name).collect::<Vec<_>>(),
                "missing_custom_predicates": missing_custom_predicates,
                "policies": policies.iter().map(policy_json).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let output = json!({
        "strict_passed": failures == 0,
        "failures": failures,
        "tables": tables,
        "schema_lints": schema_lints.iter().map(|lint| {
            json!({
                "kind": format!("{lint:?}"),
                "message": describe_schema_lint(lint),
            })
        }).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[derive(Debug, Default)]
struct CustomRegistry {
    all_ops: Vec<String>,
    rules: Vec<(String, Vec<RlsOperation>)>,
}

impl CustomRegistry {
    fn parse(all_ops: &[String], rules: &[String]) -> Result<Self> {
        let mut registry = Self {
            all_ops: all_ops.to_vec(),
            rules: Vec::new(),
        };

        for rule in rules {
            let Some((name, operations)) = rule.split_once(':') else {
                bail!("custom predicate rule `{rule}` must use NAME:OPS syntax");
            };
            let operations = operations
                .split(',')
                .map(parse_operation)
                .collect::<Result<Vec<_>>>()?;
            registry.rules.push((name.to_owned(), operations));
        }

        Ok(registry)
    }

    fn covers(&self, name: &str, operations: &[RlsOperation]) -> bool {
        self.all_ops.iter().any(|registered| registered == name)
            || self.rules.iter().any(|(registered, registered_ops)| {
                registered == name
                    && (registered_ops.is_empty()
                        || operations.is_empty()
                        || operations
                            .iter()
                            .all(|operation| registered_ops.contains(operation)))
            })
    }
}

fn parse_operation(value: &str) -> Result<RlsOperation> {
    match value {
        "select" => Ok(RlsOperation::Select),
        "insert" => Ok(RlsOperation::Insert),
        "update" => Ok(RlsOperation::Update),
        "delete" => Ok(RlsOperation::Delete),
        other => bail!("unsupported RLS operation `{other}`"),
    }
}

fn describe_schema_lint(lint: &SchemaLint) -> String {
    match lint {
        SchemaLint::UnindexedForeignKey { table, column } => {
            format!("foreign key `{table}.{column}` is not indexed")
        }
        SchemaLint::MissingRls { table } => {
            format!("table `{table}` has no RLS policy; add `rls(public)` if intentional")
        }
        SchemaLint::UnindexedRlsColumn { table, column } => {
            format!("RLS column `{table}.{column}` is not indexed")
        }
    }
}

fn dump_current_schema(project_dir: &Path) -> Result<SchemaSnapshot> {
    let src_dir = project_dir.join("src");
    let entities = discover::discover_entities(&src_dir)
        .with_context(|| format!("discovering entities under {}", src_dir.display()))?;
    schema_dump::dump_schema(project_dir, &entities)
}

fn table_failures(policies: &[OwnedRlsPolicyDef]) -> usize {
    if policies.is_empty() {
        return 1;
    }

    policies
        .iter()
        .filter(|policy| {
            matches!(policy.kind, RlsPolicyKind::Owner | RlsPolicyKind::Tenant)
                && policy.column.is_none()
                || matches!(policy.kind, RlsPolicyKind::Custom) && policy.custom.is_none()
        })
        .count()
}

fn missing_operations(policies: &[OwnedRlsPolicyDef]) -> Vec<RlsOperation> {
    if policies.is_empty()
        || policies
            .iter()
            .any(|policy| policy.kind == RlsPolicyKind::Public && policy.operations.is_empty())
    {
        return Vec::new();
    }

    [
        RlsOperation::Select,
        RlsOperation::Insert,
        RlsOperation::Update,
        RlsOperation::Delete,
    ]
    .into_iter()
    .filter(|operation| {
        policies
            .iter()
            .all(|policy| !policy.operations.is_empty() && !policy.operations.contains(operation))
    })
    .collect()
}

fn missing_custom_predicates(
    policies: &[OwnedRlsPolicyDef],
    registry: &CustomRegistry,
) -> Vec<String> {
    policies
        .iter()
        .filter(|policy| policy.kind == RlsPolicyKind::Custom)
        .filter_map(|policy| {
            let name = policy.custom.as_deref()?;
            if registry.covers(name, &policy.operations) {
                None
            } else {
                Some(name.to_owned())
            }
        })
        .collect()
}

fn operation_name(operation: &RlsOperation) -> &'static str {
    match operation {
        RlsOperation::Select => "select",
        RlsOperation::Insert => "insert",
        RlsOperation::Update => "update",
        RlsOperation::Delete => "delete",
    }
}

fn policy_json(policy: &OwnedRlsPolicyDef) -> serde_json::Value {
    json!({
        "operations": if policy.operations.is_empty() {
            vec!["all".to_owned()]
        } else {
            policy.operations.iter().map(|operation| operation_name(operation).to_owned()).collect::<Vec<_>>()
        },
        "kind": format!("{:?}", policy.kind).to_lowercase(),
        "column": policy.column.clone(),
        "custom": policy.custom.clone(),
        "authorization": {
            "mode": format!("{:?}", policy.authorization.mode),
            "roles": policy.authorization.roles.clone(),
            "permissions": policy.authorization.permissions.clone(),
            "scopes": policy.authorization.scopes.clone(),
            "resource": policy.authorization.resource.clone(),
        },
    })
}

fn describe_policy(policy: &OwnedRlsPolicyDef) -> String {
    let operations = if policy.operations.is_empty() {
        "all".to_owned()
    } else {
        policy
            .operations
            .iter()
            .map(|operation| format!("{operation:?}").to_lowercase())
            .collect::<Vec<_>>()
            .join(",")
    };

    match policy.kind {
        RlsPolicyKind::Public => format!("{operations}: public"),
        RlsPolicyKind::Owner => format!(
            "{operations}: owner column={}",
            policy.column.as_deref().unwrap_or("<missing>")
        ),
        RlsPolicyKind::Tenant => format!(
            "{operations}: tenant column={}",
            policy.column.as_deref().unwrap_or("<missing>")
        ),
        RlsPolicyKind::Rbac => format!(
            "{operations}: rbac mode={:?} roles=[{}] permissions=[{}] scopes=[{}] resource={}",
            policy.authorization.mode,
            policy.authorization.roles.join(","),
            policy.authorization.permissions.join(","),
            policy.authorization.scopes.join(","),
            policy.authorization.resource.as_deref().unwrap_or("")
        ),
        RlsPolicyKind::Custom => format!(
            "{operations}: custom name={}",
            policy.custom.as_deref().unwrap_or("<missing>")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use comet::nebula::RlsMatchMode;
    use comet::nebula::schema::OwnedRlsAuthorizationDef;

    #[test]
    fn custom_registry_covers_operation_specific_rules() {
        let registry = CustomRegistry::parse(
            &[],
            &[
                "can_complete_task:update".to_owned(),
                "can_read_task:select,insert".to_owned(),
            ],
        )
        .unwrap();

        assert!(registry.covers("can_complete_task", &[RlsOperation::Update]));
        assert!(registry.covers("can_read_task", &[RlsOperation::Select]));
        assert!(registry.covers("can_read_task", &[RlsOperation::Insert]));
        assert!(!registry.covers("can_complete_task", &[RlsOperation::Delete]));
        assert!(!registry.covers("missing", &[RlsOperation::Update]));
    }

    #[test]
    fn custom_registry_supports_legacy_all_operation_names() {
        let registry = CustomRegistry::parse(&["can_delete_task".to_owned()], &[]).unwrap();

        assert!(registry.covers("can_delete_task", &[RlsOperation::Delete]));
        assert!(registry.covers("can_delete_task", &[RlsOperation::Update]));
    }

    #[test]
    fn custom_registry_rejects_invalid_rules() {
        assert!(CustomRegistry::parse(&[], &["can_update".to_owned()]).is_err());
        assert!(CustomRegistry::parse(&[], &["can_update:publish".to_owned()]).is_err());
    }

    #[test]
    fn missing_custom_predicates_are_operation_aware() {
        let registry =
            CustomRegistry::parse(&[], &["can_complete_task:delete".to_owned()]).unwrap();
        let policies = vec![custom_policy(
            "can_complete_task",
            vec![RlsOperation::Update],
        )];

        assert_eq!(
            missing_custom_predicates(&policies, &registry),
            vec!["can_complete_task".to_owned()]
        );
    }

    #[test]
    fn policy_json_includes_machine_readable_rls_shape() {
        let policy = OwnedRlsPolicyDef {
            operations: vec![RlsOperation::Update],
            kind: RlsPolicyKind::Rbac,
            column: None,
            authorization: OwnedRlsAuthorizationDef {
                mode: RlsMatchMode::Any,
                roles: vec!["admin".to_owned()],
                permissions: vec!["tasks:write".to_owned()],
                scopes: vec!["org:current".to_owned()],
                resource: Some("task".to_owned()),
            },
            custom: None,
        };

        assert_eq!(
            policy_json(&policy),
            json!({
                "operations": ["update"],
                "kind": "rbac",
                "column": null,
                "custom": null,
                "authorization": {
                    "mode": "Any",
                    "roles": ["admin"],
                    "permissions": ["tasks:write"],
                    "scopes": ["org:current"],
                    "resource": "task",
                },
            })
        );
    }

    fn custom_policy(name: &str, operations: Vec<RlsOperation>) -> OwnedRlsPolicyDef {
        OwnedRlsPolicyDef {
            operations,
            kind: RlsPolicyKind::Custom,
            column: None,
            authorization: OwnedRlsAuthorizationDef {
                mode: RlsMatchMode::All,
                roles: Vec::new(),
                permissions: Vec::new(),
                scopes: Vec::new(),
                resource: None,
            },
            custom: Some(name.to_owned()),
        }
    }
}
