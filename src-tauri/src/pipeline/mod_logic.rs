//! Author processing script language.
//!
//! Python-like scripts with `field`, `if`/`elif`/`else`, `var`, and builtins
//! `mod` / `message` / `type`. Users write their own decrypt/extract logic.

use serde::Serialize;

use crate::pipeline::logic::{collect_fields, parse};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicFieldSpec {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub accept: Vec<String>,
    pub secret: bool,
    pub required: bool,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModLogic {
    pub fields: Vec<LogicFieldSpec>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLogicView {
    pub extract: String,
    pub place: String,
    pub match_ext: Vec<String>,
    pub match_mime: Vec<String>,
    pub fields: Vec<LogicFieldSpec>,
}

pub fn default_mod_logic_template() -> String {
    r#"field passwords
passwords.as_textarea()
if mod.get_type() == type.file:
	var f = mod.as_file()
	var e = f.ext
	if e == "rar" || e == "zip" || e == "7z":
		var result = mod.extract(passwords.split("\n"))
		if result == true:
			return true
		else:
			message = "No password works."
			return false
	return true
else:
	return true
"#
    .to_string()
}

pub(crate) fn strip_legacy_decrypt_commands(src: &str) -> String {
    src.lines()
        .filter(|raw| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                return true;
            }
            let cmd = line.split_whitespace().next().unwrap_or("");
            !cmd.eq_ignore_ascii_case("decrypt")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_mod_logic(src: &str) -> Result<ModLogic, String> {
    let stmts = parse(src)?;
    Ok(ModLogic {
        fields: collect_fields(&stmts),
    })
}

impl ModLogic {
    pub fn to_view(&self) -> ModLogicView {
        ModLogicView {
            extract: "auto".into(),
            place: "author".into(),
            match_ext: Vec::new(),
            match_mime: Vec::new(),
            fields: self.fields.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_parses_and_exposes_passwords_field() {
        let logic = parse_mod_logic(&default_mod_logic_template()).expect("parse template");
        assert_eq!(logic.fields.len(), 1);
        assert_eq!(logic.fields[0].id, "passwords");
        assert_eq!(logic.fields[0].kind, "lines");
    }
}
