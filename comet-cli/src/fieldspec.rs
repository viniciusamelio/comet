use anyhow::{Result, bail};

/// A field parsed from a `--field name:type[:attr,attr=value,...]` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    pub name: String,
    pub rust_type: String,
    /// Rendered `#[nebula(...)]` fragments, e.g. `"primary_key"` or
    /// `"default = \"0\""`, ready to join with `, `.
    pub attrs: Vec<String>,
    /// A comment to place above the field, e.g. warning about the
    /// bool-as-integer D1 quirk. Empty when there's nothing to say.
    pub comment: Option<&'static str>,
}

impl FieldSpec {
    pub fn is_primary_key(&self) -> bool {
        self.attrs.iter().any(|attr| attr == "primary_key")
    }
}

/// Parses one `--field` value. Grammar: `name:type[:attr[,attr]...]`, where
/// each `attr` is either a bare flag (`primary_key`, `auto`, `unique`,
/// `index`, `nullable`) or `key=value` (`default=0`, `rename=foo`,
/// `foreign_key=table.column`).
pub fn parse_field(spec: &str) -> Result<FieldSpec> {
    let mut parts = spec.splitn(3, ':');
    let name = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| anyhow::anyhow!("field `{spec}` is missing a name"))?;
    let ty = parts.next().ok_or_else(|| {
        anyhow::anyhow!("field `{spec}` is missing a type (expected `name:type[:attrs]`)")
    })?;
    let attrs_part = parts.next();

    let (rust_type, comment) = resolve_type_alias(ty)
        .ok_or_else(|| anyhow::anyhow!(
            "field `{name}` has unknown type `{ty}` (supported: string, i32/int, i64/bigint, f64/float, bool, bytes)"
        ))?;

    let mut attrs = Vec::new();
    if let Some(attrs_part) = attrs_part {
        for attr in attrs_part.split(',') {
            let attr = attr.trim();
            if attr.is_empty() {
                continue;
            }
            attrs.push(render_attr(attr)?);
        }
    }

    Ok(FieldSpec {
        name: name.to_owned(),
        rust_type: rust_type.to_owned(),
        attrs,
        comment,
    })
}

/// Maps a short type alias to the Rust field type to generate.
///
/// `bool` deliberately maps to `i32`, not `bool`: D1/SQLite has no boolean
/// storage class, and a `bool`-typed field can fail to deserialize a D1 row
/// that comes back as a raw `0`/`1` integer. `examples/cloudflare-worker`
/// hits this same constraint (see `TaskRow::done`) and works around it with
/// a separate public-facing type; a generic generator can't know whether a
/// caller wants that split, so it keeps the safe representation and leaves
/// the friendlier public type as a manual follow-up.
fn resolve_type_alias(ty: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match ty {
        "string" | "text" | "String" => ("String", None),
        "i32" | "int" | "integer" => ("i32", None),
        "i64" | "bigint" => ("i64", None),
        "f64" | "float" | "real" => ("f64", None),
        "bool" | "boolean" => (
            "i32",
            Some("stored as 0/1 (D1/SQLite has no boolean storage class)"),
        ),
        "bytes" | "blob" => ("Vec<u8>", None),
        _ => return None,
    })
}

fn render_attr(attr: &str) -> Result<String> {
    if let Some((key, value)) = attr.split_once('=') {
        let key = key.trim();
        let value = value.trim();
        match key {
            "default" | "rename" | "foreign_key" => {
                Ok(format!("{key} = \"{}\"", value.replace('"', "\\\"")))
            }
            other => bail!("unknown field attribute `{other}=` (with a value)"),
        }
    } else {
        match attr {
            "primary_key" | "auto" | "auto_increment" | "unique" | "index" | "indexed"
            | "nullable" => Ok(attr.to_owned()),
            other => bail!("unknown field attribute `{other}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bare_typed_field() {
        let field = parse_field("title:string").unwrap();
        assert_eq!(
            field,
            FieldSpec {
                name: "title".into(),
                rust_type: "String".into(),
                attrs: Vec::new(),
                comment: None,
            }
        );
    }

    #[test]
    fn parses_flag_and_value_attrs() {
        let field = parse_field("org_id:i64:foreign_key=orgs.id,index").unwrap();
        assert_eq!(field.rust_type, "i64");
        assert_eq!(
            field.attrs,
            vec!["foreign_key = \"orgs.id\"".to_owned(), "index".to_owned()]
        );
    }

    #[test]
    fn maps_bool_to_i32_with_a_comment() {
        let field = parse_field("done:bool").unwrap();
        assert_eq!(field.rust_type, "i32");
        assert!(field.comment.is_some());
    }

    #[test]
    fn detects_primary_key_flag() {
        let field = parse_field("id:i32:primary_key,auto,unique,index").unwrap();
        assert!(field.is_primary_key());
    }

    #[test]
    fn rejects_unknown_type() {
        let error = parse_field("title:widget").unwrap_err();
        assert!(error.to_string().contains("unknown type"));
    }

    #[test]
    fn rejects_missing_type() {
        let error = parse_field("title").unwrap_err();
        assert!(error.to_string().contains("missing a type"));
    }

    #[test]
    fn rejects_unknown_attr() {
        let error = parse_field("title:string:sparkly").unwrap_err();
        assert!(error.to_string().contains("unknown field attribute"));
    }
}
