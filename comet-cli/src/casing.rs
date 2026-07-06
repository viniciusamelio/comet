/// Converts `PascalCase`, `camelCase`, `kebab-case`, or already-`snake_case`
/// input into `snake_case`.
pub fn to_snake_case(input: &str) -> String {
    let mut result = String::new();
    let mut prev_is_lower_or_digit = false;

    for ch in input.chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            if !result.is_empty() && !result.ends_with('_') {
                result.push('_');
            }
            prev_is_lower_or_digit = false;
            continue;
        }

        if ch.is_uppercase() {
            if prev_is_lower_or_digit {
                result.push('_');
            }
            result.extend(ch.to_lowercase());
            prev_is_lower_or_digit = false;
        } else {
            result.push(ch);
            prev_is_lower_or_digit = ch.is_lowercase() || ch.is_ascii_digit();
        }
    }

    result.trim_matches('_').to_owned()
}

/// Converts input into `PascalCase` (via `snake_case` first, so any casing
/// style is accepted).
pub fn to_pascal_case(input: &str) -> String {
    to_snake_case(input)
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// A deliberately simple English pluralizer covering the common cases this
/// project's own entities need (`task` -> `tasks`, `org` -> `orgs`,
/// `entity` -> `entities`, `box` -> `boxes`). Irregular plurals aren't
/// handled — callers needing one should pass `--table` explicitly.
pub fn pluralize(word: &str) -> String {
    let ends_with_vowel_y = word.ends_with("ay")
        || word.ends_with("ey")
        || word.ends_with("iy")
        || word.ends_with("oy")
        || word.ends_with("uy");

    if word.ends_with('y') && !ends_with_vowel_y && word.len() > 1 {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

/// The `Column<T>` associated-const name the derive macro generates for a
/// field (e.g. `org_id` -> `ORG_ID`).
pub fn to_screaming_snake_case(field_name: &str) -> String {
    field_name.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_handles_pascal_and_camel_case() {
        assert_eq!(to_snake_case("BoardTask"), "board_task");
        assert_eq!(to_snake_case("boardTask"), "board_task");
        assert_eq!(to_snake_case("board_task"), "board_task");
        assert_eq!(to_snake_case("board-task"), "board_task");
        assert_eq!(to_snake_case("Board Task"), "board_task");
    }

    #[test]
    fn pascal_case_handles_any_input_casing() {
        assert_eq!(to_pascal_case("board_task"), "BoardTask");
        assert_eq!(to_pascal_case("board-task"), "BoardTask");
        assert_eq!(to_pascal_case("BoardTask"), "BoardTask");
        assert_eq!(to_pascal_case("board"), "Board");
    }

    #[test]
    fn pluralize_covers_common_endings() {
        assert_eq!(pluralize("task"), "tasks");
        assert_eq!(pluralize("org"), "orgs");
        assert_eq!(pluralize("entity"), "entities");
        assert_eq!(pluralize("box"), "boxes");
        assert_eq!(pluralize("bus"), "buses");
        assert_eq!(pluralize("day"), "days");
    }

    #[test]
    fn screaming_snake_case_just_upper_cases() {
        assert_eq!(to_screaming_snake_case("org_id"), "ORG_ID");
        assert_eq!(to_screaming_snake_case("id"), "ID");
    }
}
