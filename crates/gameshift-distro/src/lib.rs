//! Parse os-release as data. Never source it in a shell.
use std::collections::BTreeMap;

/// Decode the quoting and escaping used by os-release assignments.
pub fn parse_os_release(input: &str) -> BTreeMap<String, String> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') {
                return None;
            }
            let (key, raw) = line.split_once('=')?;
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            {
                return None;
            }
            let raw = raw.trim();
            let value = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                let mut value = String::new();
                let mut chars = raw[1..raw.len() - 1].chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '\\'
                        && chars
                            .peek()
                            .is_some_and(|n| ['"', '\\', '$', '`'].contains(n))
                    {
                        if let Some(next) = chars.next() {
                            value.push(next);
                        }
                    } else {
                        value.push(c);
                    }
                }
                value
            } else if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
                raw[1..raw.len() - 1].to_owned()
            } else {
                raw.to_owned()
            };
            Some((key.to_owned(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_without_evaluating_shell() {
        let values = parse_os_release(
            "# comment\nID=fedora\nPRETTY_NAME=\"Fedora Linux\"\nX='$(touch nope)'\nbroken\n",
        );
        assert_eq!(values["ID"], "fedora");
        assert_eq!(values["PRETTY_NAME"], "Fedora Linux");
        assert_eq!(values["X"], "$(touch nope)");
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn decodes_only_shell_escaped_special_characters() {
        let values = parse_os_release(r#"NAME="A \"quoted\" name with \\ and \$HOME and \q""#);
        assert_eq!(values["NAME"], r#"A "quoted" name with \ and $HOME and \q"#);
    }

    #[test]
    fn keeps_equals_and_uses_last_assignment() {
        let values = parse_os_release("ID=old\nID=new\nNAME='a=b'\n");
        assert_eq!(values["ID"], "new");
        assert_eq!(values["NAME"], "a=b");
    }

    #[test]
    fn ignores_invalid_keys_and_accepts_empty_values() {
        let values = parse_os_release("BAD KEY=x\n=bad\nEMPTY=\n# ID=ignored\n");
        assert_eq!(values.len(), 1);
        assert_eq!(values["EMPTY"], "");
    }
}
