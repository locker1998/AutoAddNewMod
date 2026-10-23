# AutoAddNewMod

Windows desktop app (Tauri + Vue) for importing and placing game mods from an inbox folder.

## Author custom logic

There are no built-in authors or pack decryptors. Each author has a processing script (Settings → Custom logic). You write decrypt/extract steps in that script.

The script is Python-like (indentation, `if` / `elif` / `else`, `var`). `mod` and `message` are always available — do not declare them.

After **Save**, Settings fields come from `field` lines. Hidden fields (`hide()`) are omitted from Settings.

Games whose name or id looks like Brown Dust II (`bd2`, `brown dust`, …) get Yuk11sh1d4, Na0h, and H.Coel authors with scripts that match BD2ModManager decrypt/extract behavior. You can still edit those scripts.

`decrypt <name>` is not valid. Old configs that had those lines are stripped on load. Placement uses the game setting **Organize by author**.

### Language

| Piece | Meaning |
|-------|---------|
| `field <id>` | Declares a Settings input named `<id>` |
| `if` / `elif` / `else` | Blocks end with `:` and are indented |
| `var x = …` / `x = …` | Variables and assignment |
| `return true` / `return false` | Place mods, or keep the inbox item |
| `mod` | The current inbox item |
| `message` | Text shown after process in a bottom-centered alert |
| `type.file` / `type.folder` | Values from `mod.get_type()` (archives are files) |

Comments start with `#`. `=` and `==` both mean equality in conditions. `||` / `&&` are or / and.

### Fields

Configure after `field`:

- `a.as_file()` — one file (there is no `.files()`)
- `a.as_text()` / `a.as_textarea()` / `a.as_password()`
- `a.hide()` / `a.required()`
- `a.as_file().accept({"ext":"key"})` or `{"ext":["key","jwe"],"mime":"application/octet-stream"}`

### `mod` helpers

- `mod.get_type()` → `type.file` or `type.folder`
- `mod.as_file()` / `f.ext`
- `mod.extract(passwords)` — try archive passwords in order; empty list = one no-password attempt
- `mod.write(bytes)` — replace the current file with decrypted bytes

Crypto helpers you can call yourself: `read_file`, `write_file`, `aes256_cbc_decrypt`, `aes256_gcm_decrypt`, `base64_decode`, `sha256`, `http_post`.

### Example

```text
field a
field b
a.as_file().accept({"ext":"key"})
b.as_textarea()
if mod.get_type() == type.file:
	var f = mod.as_file()
	var e = f.ext
	if e == "key":
		var k = a
		# ... use the key file to decrypt the mod, then mod.write(...)
		return true
	if e == "rar" || e == "zip":
		var passwords = b.split("\n")
		var result = mod.extract(passwords)
		if result == true:
			return true
		else:
			message = "No password works."
			return false
else:
	return true
```
