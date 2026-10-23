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

pub fn hcoel_mod_logic() -> String {
    r#"if mod.get_type() == type.file:
	var e = mod.as_file().ext
	if e == "rar" || e == "zip" || e == "7z":
		var result = mod.extract([])
		if result == true:
			return true
		else:
			message = "Could not extract archive."
			return false
	return true
else:
	return true
"#
    .to_string()
}

pub fn yuk11sh1d4_mod_logic() -> String {
    r#"field key
field passwords
key.as_file().accept({"ext":"key"}).required()
passwords.as_textarea()
if mod.get_type() == type.folder:
	return true
var f = mod.as_file()
var e = f.ext
if e == "rar" || e == "zip" || e == "7z":
	var result = mod.extract([])
	if result == true:
		return true
	result = mod.extract(passwords.split("\n"))
	if result == true:
		return true
	else:
		message = "No password works."
		return false
elif e == "modpkg":
	var kf = read_file(key)
	if len(kf) < 17:
		message = "Key file is too short"
		return false
	var master = "7b78f8476db15cb6c48c65b98e05cf3a"
	var sid_bytes = aes256_cbc_decrypt(master, kf[0:16], kf[16:])
	var sid = str(sid_bytes).trim()
	if sid == "":
		message = "Key file did not decrypt to a valid supporter id"
		return false
	var resp = http_post("https://mod-key-auth-api.onrender.com/get_token", {"supporter_id": sid, "version": "0.4.0", "targets": f.path})
	if resp.error:
		message = "Auth API error: " + str(resp.error)
		return false
	if !resp.pkg_master_key:
		message = "Auth API returned no package key"
		return false
	var pkg_key = base64_decode(resp.pkg_master_key)
	if len(pkg_key) != 32:
		message = "Unexpected pkg_master_key length"
		return false
	var data = read_file(f)
	if len(data) < 17:
		message = "modpkg file is too short"
		return false
	var pt = aes256_cbc_decrypt(pkg_key, data[0:16], data[16:])
	if len(pt) < 4:
		message = "Decrypted modpkg is empty"
		return false
	var meta_len = u32le(pt[0:4])
	if len(pt) < 4 + meta_len:
		message = "Decrypted modpkg metadata length is invalid"
		return false
	var zip_bytes = pt[4 + meta_len:]
	if zip_bytes[0:2] != "PK":
		message = "Decrypted modpkg did not contain a zip payload"
		return false
	mod.write(zip_bytes)
	var unpacked = mod.extract([])
	if unpacked == true:
		return true
	else:
		message = "Decrypted payload is not a valid zip"
		return false
else:
	return true
"#
    .to_string()
}

pub fn na0h_mod_logic() -> String {
    r#"field token
token.as_file().accept({"ext":"jwe"}).required()
if mod.get_type() == type.folder:
	return true
var f = mod.as_file()
var e = f.ext
if e == "rar" || e == "zip" || e == "7z":
	var result = mod.extract([])
	if result == true:
		return true
	else:
		message = "Could not extract archive."
		return false
elif e == "encrypt":
	var jwe = str(read_file(token)).trim()
	if jwe == "":
		message = "Na0h token file is empty"
		return false
	var resp = http_post("http://138.2.152.16:5000/BD2DecryptorKey", {}, {"Authorization": "Bearer " + jwe})
	if resp.error:
		message = "Na0h key server error: " + str(resp.error)
		return false
	if resp.ok != true:
		message = "Na0h key server request failed"
		return false
	if !resp.key:
		message = "Na0h key server response missing key"
		return false
	var raw = base64_decode(resp.key)
	var aes_key = hex_decode(xor(raw, "id7!?uo-_d2%_A!q_8ZtOl_4!s1a"))
	if len(aes_key) != 32:
		message = "Unexpected Na0h AES key length"
		return false
	var data = read_file(f)
	if len(data) < 51 || data[0:6] != "BD2NA0":
		message = "Not a valid Na0h ENCRYPT file"
		return false
	var chunk_size = u32be(data[7:11])
	var ct_len = u64be(data[15:23])
	var iv = data[23:35]
	var tag = data[35:51]
	if len(data) < 51 + ct_len:
		message = "ENCRYPT file is truncated"
		return false
	var ct = data[51:51 + ct_len]
	var unshuffled = unshuffle(ct, chunk_size, aes_key)
	var plaintext = aes256_gcm_decrypt(aes_key, iv, unshuffled, tag)
	if plaintext[0:2] != "PK":
		message = "Decrypted ENCRYPT did not contain a zip payload"
		return false
	mod.write(plaintext)
	var unpacked = mod.extract([])
	if unpacked == true:
		return true
	else:
		message = "Decrypted payload is not a valid zip"
		return false
else:
	return true
"#
    .to_string()
}

#[derive(Debug, Clone)]
pub struct AuthorPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub logic: fn() -> String,
}

pub fn bd2_author_presets() -> &'static [AuthorPreset] {
    &[
        AuthorPreset {
            id: "yuk11sh1d4",
            name: "Yuk11sh1d4",
            aliases: &["yuk", "yuk11sh1d4"],
            logic: yuk11sh1d4_mod_logic,
        },
        AuthorPreset {
            id: "na0h",
            name: "Na0h",
            aliases: &["na0h", "naoh"],
            logic: na0h_mod_logic,
        },
        AuthorPreset {
            id: "hcoel",
            name: "H.Coel",
            aliases: &["hcoel", "h-coel"],
            logic: hcoel_mod_logic,
        },
    ]
}

pub fn is_bd2_game_label(id: &str, name: &str) -> bool {
    let compact: String = format!("{id} {name}")
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    compact.contains("bd2") || compact.contains("browndust")
}

pub fn is_generic_mod_logic(src: &str) -> bool {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed == default_mod_logic_template().trim() {
        return true;
    }
    if parse_mod_logic(trimmed).is_err() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("extract auto") || lower.contains("place author")
}

fn ident_key(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub fn preset_matches(preset: &AuthorPreset, id: &str, name: &str) -> bool {
    let keys = [ident_key(id), ident_key(name)];
    let mut aliases: Vec<String> = preset.aliases.iter().map(|a| ident_key(a)).collect();
    aliases.push(ident_key(preset.id));
    aliases.push(ident_key(preset.name));
    keys.iter().any(|k| !k.is_empty() && aliases.iter().any(|a| a == k))
}

pub(crate) fn former_builtin_author_logic(id: &str, name: &str) -> Option<String> {
    bd2_author_presets()
        .iter()
        .find(|p| preset_matches(p, id, name))
        .map(|p| (p.logic)())
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

    #[test]
    fn bd2_author_scripts_parse() {
        let yuk = parse_mod_logic(&yuk11sh1d4_mod_logic()).expect("yuk");
        assert!(yuk.fields.iter().any(|f| f.id == "key" && f.kind == "file" && f.required));
        assert!(yuk.fields.iter().any(|f| f.id == "passwords" && f.kind == "lines"));

        let na0h = parse_mod_logic(&na0h_mod_logic()).expect("na0h");
        assert!(na0h.fields.iter().any(|f| f.id == "token" && f.kind == "file" && f.required));

        parse_mod_logic(&hcoel_mod_logic()).expect("hcoel");
    }

    #[test]
    fn detects_bd2_game_labels() {
        assert!(is_bd2_game_label("brown-dust-ii", "Brown Dust II"));
        assert!(is_bd2_game_label("bd2", "BD2"));
        assert!(!is_bd2_game_label("other", "Some Game"));
    }
}
