use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aes::Aes256;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use cbc::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit},
    Decryptor,
};
use sha2::{Digest, Sha256};

use crate::pipeline::extract::extract_mods_password_order;
use crate::pipeline::logic::ast::{BinOp, Expr, Stmt};
use crate::pipeline::mod_logic::LogicFieldSpec;

type Aes256CbcDec = Decryptor<Aes256>;

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(HashMap<String, Value>),
    Type(&'static str),
    File {
        path: String,
    },
    Field {
        id: String,
    },
    Mod,
    TypeNs,
}

impl Value {
    fn ty_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "list",
            Value::Map(_) => "object",
            Value::Type(_) => "type",
            Value::File { .. } => "file",
            Value::Field { .. } => "field",
            Value::Mod => "mod",
            Value::TypeNs => "type",
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::List(v) => !v.is_empty(),
            _ => true,
        }
    }

    fn as_string(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Value::String(s) => s.clone(),
            Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            Value::File { path } => path.clone(),
            Value::Field { id } => id.clone(),
            Value::Type(t) => (*t).to_string(),
            _ => self.ty_name().to_string(),
        }
    }

    fn as_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Value::Bytes(b) => Ok(b.clone()),
            Value::String(s) => Ok(s.as_bytes().to_vec()),
            Value::List(items) => {
                let mut out = Vec::new();
                for item in items {
                    match item {
                        Value::Number(n) => out.push(*n as u8),
                        Value::Bytes(b) => out.extend_from_slice(b),
                        Value::String(s) => out.extend_from_slice(s.as_bytes()),
                        _ => return Err("bytes list items must be numbers or bytes".into()),
                    }
                }
                Ok(out)
            }
            _ => Err(format!("cannot convert {} to bytes", self.ty_name())),
        }
    }

    fn eq_value(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => (a - b).abs() < f64::EPSILON,
            (Value::Type(a), Value::Type(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::File { path: a }, Value::File { path: b }) => a == b,
            (Value::File { path }, Value::String(s)) | (Value::String(s), Value::File { path }) => {
                path == s
            }
            (Value::Bytes(b), Value::String(s)) | (Value::String(s), Value::Bytes(b)) => {
                b.as_slice() == s.as_bytes()
            }
            (Value::Type(t), Value::String(s)) | (Value::String(s), Value::Type(t)) => s == *t,
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_value(y))
            }
            _ => false,
        }
    }
}

enum Flow {
    Next,
    Return(Value),
}

pub struct ScriptResult {
    pub success: bool,
    pub message: Option<String>,
    pub extracted: Vec<String>,
    pub did_extract: bool,
}

pub struct LogicHost<'a> {
    pub work: PathBuf,
    pub field_values: &'a HashMap<String, String>,
    pub extra_password: Option<String>,
    current: PathBuf,
    message: Option<String>,
    extracted: Vec<String>,
    did_extract: bool,
}

impl<'a> LogicHost<'a> {
    pub fn new(
        path: PathBuf,
        work: PathBuf,
        field_values: &'a HashMap<String, String>,
        extra_password: Option<String>,
    ) -> Self {
        Self {
            work,
            field_values,
            extra_password,
            current: path,
            message: None,
            extracted: Vec::new(),
            did_extract: false,
        }
    }
}

pub fn collect_fields(stmts: &[Stmt]) -> Vec<LogicFieldSpec> {
    let mut fields: HashMap<String, LogicFieldSpec> = HashMap::new();
    let mut order = Vec::new();
    walk_fields(stmts, &mut fields, &mut order);
    order
        .into_iter()
        .filter_map(|id| fields.remove(&id))
        .collect()
}

fn walk_fields(
    stmts: &[Stmt],
    fields: &mut HashMap<String, LogicFieldSpec>,
    order: &mut Vec<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Field { name, .. } => {
                if !fields.contains_key(name) {
                    order.push(name.clone());
                    fields.insert(
                        name.clone(),
                        LogicFieldSpec {
                            id: name.clone(),
                            kind: "text".into(),
                            label: name.clone(),
                            accept: Vec::new(),
                            secret: false,
                            required: false,
                            hidden: false,
                        },
                    );
                }
            }
            Stmt::Expr { expr, .. } => apply_field_setup(expr, fields),
            Stmt::If {
                body,
                elifs,
                else_body,
                ..
            } => {
                walk_fields(body, fields, order);
                for (_, b) in elifs {
                    walk_fields(b, fields, order);
                }
                if let Some(b) = else_body {
                    walk_fields(b, fields, order);
                }
            }
            _ => {}
        }
    }
}

fn apply_field_setup(expr: &Expr, fields: &mut HashMap<String, LogicFieldSpec>) {
    if let Expr::Call { callee, args } = expr {
        apply_field_setup(callee, fields);
        if let Expr::Prop { object, name } = callee.as_ref() {
            if let Some(id) = root_ident(object) {
                if let Some(field) = fields.get_mut(&id) {
                    match name.as_str() {
                        "as_file" => field.kind = "file".into(),
                        "as_text" => field.kind = "text".into(),
                        "as_textarea" => field.kind = "lines".into(),
                        "as_password" => {
                            field.kind = "password".into();
                            field.secret = true;
                        }
                        "hide" => field.hidden = true,
                        "required" => field.required = true,
                        "accept" => {
                            if let Some(Expr::Map(pairs)) = args.first() {
                                for (k, v) in pairs {
                                    if k == "ext" {
                                        field.accept.extend(exts_from_expr(v));
                                    } else if k == "mime" {
                                        field.accept.extend(mimes_from_expr(v));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    } else if let Expr::Prop { object, .. } = expr {
        apply_field_setup(object, fields);
    }
}

fn root_ident(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(s) => Some(s.clone()),
        Expr::Prop { object, .. } | Expr::Call { callee: object, .. } => root_ident(object),
        _ => None,
    }
}

fn exts_from_expr(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::String(s) => vec![normalize_ext(s)],
        Expr::List(items) => items
            .iter()
            .filter_map(|i| match i {
                Expr::String(s) => Some(normalize_ext(s)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn mimes_from_expr(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::String(s) => vec![s.clone()],
        Expr::List(items) => items
            .iter()
            .filter_map(|i| match i {
                Expr::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_ext(s: &str) -> String {
    let t = s.trim().trim_start_matches('.').to_ascii_lowercase();
    if t.is_empty() {
        t
    } else {
        format!(".{t}")
    }
}

pub fn run(stmts: &[Stmt], host: &mut LogicHost<'_>) -> Result<ScriptResult, String> {
    let fields_vec = collect_fields(stmts);
    let mut fields = HashMap::new();
    let mut vars = HashMap::new();
    vars.insert("mod".into(), Value::Mod);
    vars.insert("type".into(), Value::TypeNs);
    for spec in &fields_vec {
        fields.insert(spec.id.clone(), spec.clone());
        vars.insert(
            spec.id.clone(),
            Value::Field {
                id: spec.id.clone(),
            },
        );
    }

    let flow = exec_block(stmts, &mut vars, &fields, host)?;
    let success = match flow {
        Flow::Next => true,
        Flow::Return(v) => resolve_value(&v, &vars, &fields, host).as_bool(),
    };

    Ok(ScriptResult {
        success,
        message: host.message.clone().filter(|s| !s.is_empty()),
        extracted: host.extracted.clone(),
        did_extract: host.did_extract,
    })
}

fn exec_block(
    stmts: &[Stmt],
    vars: &mut HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) -> Result<Flow, String> {
    for stmt in stmts {
        match exec_stmt(stmt, vars, fields, host)? {
            Flow::Next => {}
            other => return Ok(other),
        }
    }
    Ok(Flow::Next)
}

fn exec_stmt(
    stmt: &Stmt,
    vars: &mut HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) -> Result<Flow, String> {
    let line = stmt_line(stmt);
    exec_stmt_inner(stmt, vars, fields, host).map_err(|e| annotate_line(line, e))
}

fn stmt_line(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Field { line, .. }
        | Stmt::Var { line, .. }
        | Stmt::Assign { line, .. }
        | Stmt::If { line, .. }
        | Stmt::Return { line, .. }
        | Stmt::Expr { line, .. } => *line,
    }
}

fn annotate_line(line: usize, err: String) -> String {
    if err.starts_with("Line ") {
        err
    } else {
        format!("Line {line}: {err}")
    }
}

fn exec_stmt_inner(
    stmt: &Stmt,
    vars: &mut HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) -> Result<Flow, String> {
    match stmt {
        Stmt::Field { .. } => Ok(Flow::Next),
        Stmt::Expr { expr, .. } => {
            let _ = eval_expr(expr, vars, fields, host)?;
            Ok(Flow::Next)
        }
        Stmt::Var { name, value, .. } | Stmt::Assign { name, value, .. } => {
            let v = eval_expr(value, vars, fields, host)?;
            assign(name, v, vars, fields, host);
            Ok(Flow::Next)
        }
        Stmt::Return { value, .. } => {
            let v = eval_expr(value, vars, fields, host)?;
            Ok(Flow::Return(resolve_value(&v, vars, fields, host)))
        }
        Stmt::If {
            cond,
            body,
            elifs,
            else_body,
            ..
        } => {
            if eval_truthy(cond, vars, fields, host)? {
                return exec_block(body, vars, fields, host);
            }
            for (c, b) in elifs {
                if eval_truthy(c, vars, fields, host)? {
                    return exec_block(b, vars, fields, host);
                }
            }
            if let Some(b) = else_body {
                return exec_block(b, vars, fields, host);
            }
            Ok(Flow::Next)
        }
    }
}

fn assign(
    name: &str,
    v: Value,
    vars: &mut HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) {
    let v = resolve_value(&v, vars, fields, host);
    if name == "message" {
        host.message = Some(v.as_string());
    }
    vars.insert(name.to_string(), v);
}

fn lookup(
    name: &str,
    vars: &HashMap<String, Value>,
    _fields: &HashMap<String, LogicFieldSpec>,
    host: &LogicHost<'_>,
) -> Result<Value, String> {
    if name == "message" {
        return Ok(Value::String(
            host.message.clone().unwrap_or_default(),
        ));
    }
    let v = vars
        .get(name)
        .cloned()
        .ok_or_else(|| format!("unknown name `{name}`"))?;
    Ok(v)
}

fn resolve_value(
    v: &Value,
    _vars: &HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &LogicHost<'_>,
) -> Value {
    match v {
        Value::Field { id } => {
            let raw = host.field_values.get(id).cloned().unwrap_or_default();
            let kind = fields.get(id).map(|f| f.kind.as_str()).unwrap_or("text");
            if kind == "file" {
                Value::File { path: raw }
            } else {
                Value::String(raw)
            }
        }
        other => other.clone(),
    }
}

fn eval_truthy(
    expr: &Expr,
    vars: &mut HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) -> Result<bool, String> {
    let v = eval_expr(expr, vars, fields, host)?;
    Ok(resolve_value(&v, vars, fields, host).as_bool())
}

fn eval_expr(
    expr: &Expr,
    vars: &mut HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) -> Result<Value, String> {
    match expr {
        Expr::Null => Ok(Value::Null),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::String(s) => Ok(Value::String(s.clone())),
        Expr::Ident(name) => lookup(name, vars, fields, host),
        Expr::List(items) => {
            let mut out = Vec::new();
            for item in items {
                out.push(eval_expr(item, vars, fields, host)?);
            }
            Ok(Value::List(out))
        }
        Expr::Map(pairs) => {
            let mut map = HashMap::new();
            for (k, v) in pairs {
                map.insert(k.clone(), eval_expr(v, vars, fields, host)?);
            }
            Ok(Value::Map(map))
        }
        Expr::UnaryNot(inner) => {
            let v = eval_expr(inner, vars, fields, host)?;
            Ok(Value::Bool(!resolve_value(&v, vars, fields, host).as_bool()))
        }
        Expr::Binary { op, left, right } => {
            if matches!(op, BinOp::And | BinOp::Or) {
                let l = resolve_value(
                    &eval_expr(left, vars, fields, host)?,
                    vars,
                    fields,
                    host,
                );
                return match op {
                    BinOp::And => {
                        if !l.as_bool() {
                            Ok(Value::Bool(false))
                        } else {
                            Ok(Value::Bool(eval_truthy(right, vars, fields, host)?))
                        }
                    }
                    BinOp::Or => {
                        if l.as_bool() {
                            Ok(Value::Bool(true))
                        } else {
                            Ok(Value::Bool(eval_truthy(right, vars, fields, host)?))
                        }
                    }
                    _ => unreachable!(),
                };
            }
            let l = resolve_value(
                &eval_expr(left, vars, fields, host)?,
                vars,
                fields,
                host,
            );
            let r = resolve_value(
                &eval_expr(right, vars, fields, host)?,
                vars,
                fields,
                host,
            );
            binop(*op, &l, &r)
        }
        Expr::Prop { object, name } => {
            let obj = eval_expr(object, vars, fields, host)?;
            prop(&obj, name, fields, host)
        }
        Expr::Call { callee, args } => {
            let mut argv = Vec::new();
            for a in args {
                let v = eval_expr(a, vars, fields, host)?;
                argv.push(resolve_value(&v, vars, fields, host));
            }
            match callee.as_ref() {
                Expr::Ident(name) => call_func(name, &argv),
                Expr::Prop { object, name } => {
                    let obj = eval_expr(object, vars, fields, host)?;
                    call_method(&obj, name, &argv, vars, fields, host)
                }
                _ => {
                    let c = eval_expr(callee, vars, fields, host)?;
                    Err(format!("cannot call {}", c.ty_name()))
                }
            }
        }
        Expr::Index { object, index, end } => {
            let obj = eval_expr(object, vars, fields, host)?;
            let start = eval_expr(index, vars, fields, host)?;
            let end_v = match end {
                Some(e) => Some(eval_expr(e, vars, fields, host)?),
                None => None,
            };
            index_value(&obj, &start, end_v.as_ref())
        }
    }
}

fn binop(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    match op {
        BinOp::Eq => Ok(Value::Bool(l.eq_value(r))),
        BinOp::NotEq => Ok(Value::Bool(!l.eq_value(r))),
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            let (Value::Number(a), Value::Number(b)) = (l, r) else {
                return Err("numeric comparison expected".into());
            };
            Ok(Value::Bool(match op {
                BinOp::Lt => a < b,
                BinOp::Gt => a > b,
                BinOp::Le => a <= b,
                BinOp::Ge => a >= b,
                _ => false,
            }))
        }
        BinOp::Add => match (l, r) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
            (Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b.as_string()))),
            (a, Value::String(b)) => Ok(Value::String(format!("{}{}", a.as_string(), b))),
            (Value::Bytes(a), Value::Bytes(b)) => {
                let mut o = a.clone();
                o.extend_from_slice(b);
                Ok(Value::Bytes(o))
            }
            (Value::List(a), Value::List(b)) => {
                let mut o = a.clone();
                o.extend(b.iter().cloned());
                Ok(Value::List(o))
            }
            _ => Err("cannot add these values".into()),
        },
        BinOp::Sub => num2(l, r, |a, b| a - b),
        BinOp::Mul => num2(l, r, |a, b| a * b),
        BinOp::Div => num2(l, r, |a, b| a / b),
        BinOp::And | BinOp::Or => unreachable!(),
    }
}

fn num2(l: &Value, r: &Value, f: impl Fn(f64, f64) -> f64) -> Result<Value, String> {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => Ok(Value::Number(f(*a, *b))),
        _ => Err("numeric operation expected".into()),
    }
}

fn prop(
    obj: &Value,
    name: &str,
    _fields: &HashMap<String, LogicFieldSpec>,
    host: &LogicHost<'_>,
) -> Result<Value, String> {
    match obj {
        Value::TypeNs => match name {
            "file" => Ok(Value::Type("file")),
            "folder" => Ok(Value::Type("folder")),
            "archive" => Ok(Value::Type("archive")),
            _ => Err(format!("unknown type.{name}")),
        },
        Value::File { path } => file_prop(path, name),
        Value::Mod => match name {
            "path" => Ok(Value::String(host.current.to_string_lossy().into_owned())),
            "name" => Ok(Value::String(
                host.current
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )),
            "ext" => file_prop(&host.current.to_string_lossy(), "ext"),
            _ => Err(format!("unknown mod.{name}")),
        },
        Value::Map(map) => Ok(map.get(name).cloned().unwrap_or(Value::Null)),
        Value::String(s) if Path::new(s).exists() => file_prop(s, name),
        _ => Err(format!("{} has no property `{name}`", obj.ty_name())),
    }
}

fn file_prop(path: &str, name: &str) -> Result<Value, String> {
    let p = Path::new(path);
    match name {
        "path" => Ok(Value::String(path.to_string())),
        "name" => Ok(Value::String(
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )),
        "ext" | "extension" => Ok(Value::String(
            p.extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default(),
        )),
        "size" => Ok(Value::Number(
            std::fs::metadata(p).map(|m| m.len() as f64).unwrap_or(0.0),
        )),
        _ => Err(format!("unknown file.{name}")),
    }
}

fn call_func(name: &str, args: &[Value]) -> Result<Value, String> {
    match name {
        "len" => {
            let v = args.first().ok_or("len() needs a value")?;
            let n = match v {
                Value::String(s) => s.len(),
                Value::Bytes(b) => b.len(),
                Value::List(l) => l.len(),
                _ => return Err("len() expected string, bytes, or list".into()),
            };
            Ok(Value::Number(n as f64))
        }
        "str" => Ok(Value::String(
            args.first().unwrap_or(&Value::Null).as_string(),
        )),
        "read_file" => {
            let path = args
                .first()
                .and_then(|v| match v {
                    Value::String(s) | Value::File { path: s } => Some(s.clone()),
                    other => {
                        let s = other.as_string();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s)
                        }
                    }
                })
                .ok_or("read_file() needs a path")?;
            let data = std::fs::read(&path).map_err(|e| format!("read_file failed: {e}"))?;
            Ok(Value::Bytes(data))
        }
        "write_file" => {
            let path = args.first().and_then(|v| match v {
                Value::String(s) | Value::File { path: s } => Some(s.clone()),
                _ => None,
            }).ok_or("write_file() needs a path")?;
            let data = args.get(1).ok_or("write_file() needs data")?.as_bytes()?;
            if let Some(parent) = Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&path, data).map_err(|e| format!("write_file failed: {e}"))?;
            Ok(Value::Bool(true))
        }
        "aes256_cbc_decrypt" => {
            let key = args.first().ok_or("missing key")?.as_bytes()?;
            let iv = args.get(1).ok_or("missing iv")?.as_bytes()?;
            let data = args.get(2).ok_or("missing data")?.as_bytes()?;
            aes_cbc(&key, &iv, &data).map(Value::Bytes)
        }
        "aes256_gcm_decrypt" => {
            let key = args.first().ok_or("missing key")?.as_bytes()?;
            let nonce = args.get(1).ok_or("missing nonce")?.as_bytes()?;
            let data = args.get(2).ok_or("missing data")?.as_bytes()?;
            let tag = args.get(3).ok_or("missing tag")?.as_bytes()?;
            aes_gcm(&key, &nonce, &data, &tag).map(Value::Bytes)
        }
        "base64_decode" => {
            let s = args.first().ok_or("base64_decode needs a string")?.as_string();
            let raw = B64.decode(s.trim()).map_err(|e| format!("base64_decode: {e}"))?;
            Ok(Value::Bytes(raw))
        }
        "sha256" => {
            let data = args.first().ok_or("sha256 needs data")?.as_bytes()?;
            let mut hasher = Sha256::new();
            hasher.update(&data);
            Ok(Value::Bytes(hasher.finalize().to_vec()))
        }
        "http_post" => {
            let url = args.first().ok_or("http_post needs url")?.as_string();
            let body = args.get(1).cloned().unwrap_or(Value::Map(HashMap::new()));
            http_post(&url, &body, args.get(2))
        }
        "bytes" => args
            .first()
            .ok_or_else(|| "bytes() needs a value".into())
            .and_then(Value::as_bytes)
            .map(Value::Bytes),
        "u32le" => int_from_bytes(args.first(), true, 4),
        "u32be" => int_from_bytes(args.first(), false, 4),
        "u64be" => int_from_bytes(args.first(), false, 8),
        "xor" => {
            let data = args.first().ok_or("xor needs data")?.as_bytes()?;
            let mask = args.get(1).ok_or("xor needs a mask")?.as_bytes()?;
            if mask.is_empty() {
                return Err("xor mask is empty".into());
            }
            Ok(Value::Bytes(
                data.iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % mask.len()])
                    .collect(),
            ))
        }
        "hex_decode" => {
            let raw = args.first().ok_or("hex_decode needs data")?.as_bytes()?;
            hex_decode(&raw).map(Value::Bytes)
        }
        "unshuffle" => {
            let ct = args.first().ok_or("unshuffle needs data")?.as_bytes()?;
            let chunk = match args.get(1) {
                Some(Value::Number(n)) if *n > 0.0 => *n as usize,
                _ => return Err("unshuffle needs a positive chunk size".into()),
            };
            let key = args.get(2).ok_or("unshuffle needs a key")?.as_bytes()?;
            unshuffle(&ct, chunk, &key).map(Value::Bytes)
        }
        _ => Err(format!("unknown function `{name}`")),
    }
}

fn call_method(
    obj: &Value,
    name: &str,
    args: &[Value],
    _vars: &HashMap<String, Value>,
    fields: &HashMap<String, LogicFieldSpec>,
    host: &mut LogicHost<'_>,
) -> Result<Value, String> {
    if matches!(
        name,
        "as_file" | "as_text" | "as_textarea" | "as_password" | "hide" | "required" | "accept"
    ) {
        if let Value::Field { id } = obj {
            return Ok(Value::Field { id: id.clone() });
        }
    }
    if let Value::Field { id: _ } = obj {
        let resolved = resolve_value(obj, _vars, fields, host);
        return call_method(&resolved, name, args, _vars, fields, host);
    }

    match (obj, name) {
        (Value::Mod, "get_type") => {
            if host.current.is_dir() {
                Ok(Value::Type("folder"))
            } else {
                Ok(Value::Type("file"))
            }
        }
        (Value::Mod, "as_file") => {
            if host.current.is_dir() {
                Err("mod is a folder".into())
            } else {
                Ok(Value::File {
                    path: host.current.to_string_lossy().into_owned(),
                })
            }
        }
        (Value::Mod, "as_folder") => {
            if host.current.is_dir() {
                Ok(Value::File {
                    path: host.current.to_string_lossy().into_owned(),
                })
            } else {
                Err("mod is not a folder".into())
            }
        }
        (Value::Mod, "extract") => {
            let mut passwords = passwords_from(args.first());
            if let Some(extra) = &host.extra_password {
                if !extra.is_empty() && !passwords.iter().any(|p| p == extra) {
                    passwords.insert(0, extra.clone());
                }
            }
            match extract_mods_password_order(&host.current, &host.work, &passwords) {
                Ok(names) => {
                    host.extracted = names;
                    host.did_extract = true;
                    Ok(Value::Bool(true))
                }
                Err(_) => Ok(Value::Bool(false)),
            }
        }
        (Value::Mod, "write") => {
            let data = args.first().ok_or("mod.write needs data")?.as_bytes()?;
            let name = if data.starts_with(b"PK") {
                "decrypted.zip"
            } else {
                "decrypted.bin"
            };
            let out = host.work.join(name);
            std::fs::write(&out, data).map_err(|e| format!("mod.write failed: {e}"))?;
            host.current = out;
            Ok(Value::Bool(true))
        }
        (Value::String(s), "split") => {
            let sep = args.first().map(Value::as_string).unwrap_or_else(|| "\n".into());
            Ok(Value::List(
                s.split(&sep)
                    .map(|p| Value::String(p.to_string()))
                    .collect(),
            ))
        }
        (Value::String(s), "trim") => Ok(Value::String(s.trim().to_string())),
        (Value::String(s), "lower") => Ok(Value::String(s.to_lowercase())),
        (Value::String(s), "upper") => Ok(Value::String(s.to_uppercase())),
        (Value::String(s), "ends_with") => {
            let suf = args.first().map(Value::as_string).unwrap_or_default();
            Ok(Value::Bool(s.ends_with(&suf)))
        }
        (Value::String(s), "starts_with") => {
            let pre = args.first().map(Value::as_string).unwrap_or_default();
            Ok(Value::Bool(s.starts_with(&pre)))
        }
        (Value::File { path }, "read") => {
            let data = std::fs::read(path).map_err(|e| format!("file.read failed: {e}"))?;
            Ok(Value::Bytes(data))
        }
        (Value::File { path }, "as_file") => Ok(Value::File { path: path.clone() }),
        (Value::String(s), "as_file") => Ok(Value::File { path: s.clone() }),
        _ => Err(format!(
            "{} has no method `{name}`",
            obj.ty_name()
        )),
    }
}

fn passwords_from(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::List(items)) => items
            .iter()
            .map(Value::as_string)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => s
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Some(other) => {
            let s = other.as_string();
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s]
            }
        }
    }
}

fn index_value(obj: &Value, start: &Value, end: Option<&Value>) -> Result<Value, String> {
    let start_i = match start {
        Value::Number(n) => *n as i64,
        Value::Null => 0,
        _ => return Err("index must be a number".into()),
    };
    match obj {
        Value::Bytes(b) => slice_bytes(b, start_i, end),
        Value::String(s) => {
            let bytes = s.as_bytes();
            match slice_bytes(bytes, start_i, end)? {
                Value::Bytes(b) => Ok(Value::String(String::from_utf8_lossy(&b).into_owned())),
                other => Ok(other),
            }
        }
        Value::List(items) => {
            let len = items.len() as i64;
            let s = norm_index(start_i, len);
            if let Some(e) = end {
                let end_i = match e {
                    Value::Null => len,
                    Value::Number(n) => *n as i64,
                    _ => return Err("slice end must be a number".into()),
                };
                let e = norm_index(end_i, len).min(len) as usize;
                let s = s.max(0) as usize;
                Ok(Value::List(items.get(s..e).unwrap_or(&[]).to_vec()))
            } else {
                let i = s as usize;
                items
                    .get(i)
                    .cloned()
                    .ok_or_else(|| "index out of range".into())
            }
        }
        _ => Err("value is not indexable".into()),
    }
}

fn slice_bytes(b: &[u8], start_i: i64, end: Option<&Value>) -> Result<Value, String> {
    let len = b.len() as i64;
    let s = norm_index(start_i, len);
    if let Some(e) = end {
        let end_i = match e {
            Value::Null => len,
            Value::Number(n) => *n as i64,
            _ => return Err("slice end must be a number".into()),
        };
        let e = norm_index(end_i, len).min(len) as usize;
        let s = s.max(0) as usize;
        Ok(Value::Bytes(b.get(s..e).unwrap_or(&[]).to_vec()))
    } else {
        let i = s as usize;
        b.get(i)
            .map(|n| Value::Number(*n as f64))
            .ok_or_else(|| "index out of range".into())
    }
}

fn norm_index(i: i64, len: i64) -> i64 {
    if i < 0 {
        (len + i).max(0)
    } else {
        i
    }
}

fn aes_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 32 {
        return Err(format!("AES-CBC key must be 32 bytes, got {}", key.len()));
    }
    if iv.len() != 16 {
        return Err("AES-CBC IV must be 16 bytes".into());
    }
    let mut buf = data.to_vec();
    let dec = Aes256CbcDec::new_from_slices(key, iv).map_err(|e| e.to_string())?;
    let pt = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(pt.to_vec())
}

fn aes_gcm(key: &[u8], nonce: &[u8], data: &[u8], tag: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 32 {
        return Err(format!("AES-GCM key must be 32 bytes, got {}", key.len()));
    }
    if nonce.len() != 12 {
        return Err("AES-GCM nonce must be 12 bytes".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let n = Nonce::from_slice(nonce);
    let mut packed = Vec::with_capacity(data.len() + tag.len());
    packed.extend_from_slice(data);
    packed.extend_from_slice(tag);
    cipher
        .decrypt(n, packed.as_ref())
        .map_err(|_| "AES-GCM decrypt failed".into())
}

fn int_from_bytes(v: Option<&Value>, le: bool, n: usize) -> Result<Value, String> {
    let bytes = v.ok_or("integer conversion needs bytes")?.as_bytes()?;
    if bytes.len() < n {
        return Err(format!("need {n} bytes, got {}", bytes.len()));
    }
    let slice = &bytes[..n];
    let val = match (n, le) {
        (4, true) => u32::from_le_bytes(slice.try_into().unwrap()) as f64,
        (4, false) => u32::from_be_bytes(slice.try_into().unwrap()) as f64,
        (8, false) => u64::from_be_bytes(slice.try_into().unwrap()) as f64,
        (8, true) => u64::from_le_bytes(slice.try_into().unwrap()) as f64,
        _ => return Err("unsupported integer width".into()),
    };
    Ok(Value::Number(val))
}

fn hex_decode(hex: &[u8]) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("hex_decode: odd length".into());
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks_exact(2) {
        let s = std::str::from_utf8(pair).map_err(|_| "hex_decode: not ASCII".to_string())?;
        out.push(u8::from_str_radix(s, 16).map_err(|_| "hex_decode: invalid hex".to_string())?);
    }
    Ok(out)
}

fn unshuffle(ct: &[u8], chunk_size: usize, aes_key: &[u8]) -> Result<Vec<u8>, String> {
    if chunk_size == 0 {
        return Err("unshuffle: chunk size is 0".into());
    }
    let chunks: Vec<&[u8]> = ct.chunks(chunk_size).collect();
    if chunks.is_empty() {
        return Err("unshuffle: ciphertext is empty".into());
    }
    let last = *chunks.last().unwrap();
    let head = &chunks[..chunks.len() - 1];
    let n = head.len();
    if n == 0 {
        return Ok(last.to_vec());
    }

    let mut hasher = Sha256::new();
    hasher.update(b"perm");
    hasher.update(aes_key);
    let digest = hasher.finalize();
    let seed = u64::from_be_bytes(digest[..8].try_into().unwrap());
    let mut arr: Vec<usize> = (0..n).collect();
    let mut state = seed;
    for i in (1..n).rev() {
        state = state.wrapping_mul(0x5852_21a6_d4cf_8a1d).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        arr.swap(i, j);
    }

    let mut out: Vec<&[u8]> = vec![&[]; n];
    for (i, &p) in arr.iter().enumerate() {
        if p >= n {
            return Err("unshuffle: invalid permutation".into());
        }
        out[p] = head[i];
    }

    let mut plain = Vec::with_capacity(ct.len());
    for part in out {
        plain.extend_from_slice(part);
    }
    plain.extend_from_slice(last);
    Ok(plain)
}

fn http_post(url: &str, body: &Value, headers: Option<&Value>) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let json = value_to_json(body);
    let mut req = client.post(url).json(&json);
    if let Some(Value::Map(map)) = headers {
        for (k, v) in map {
            req = req.header(k.as_str(), v.as_string());
        }
    }
    let resp = req.send().map_err(|e| format!("http_post failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        return Ok(json_to_value(&v));
    }
    if !status.is_success() {
        return Err(format!("http_post HTTP {status}: {text}"));
    }
    Ok(Value::String(text))
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => serde_json::json!(n),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                obj.insert(k.clone(), value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        other => serde_json::Value::String(other.as_string()),
    }
}

fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(items) => Value::List(items.iter().map(json_to_value).collect()),
        serde_json::Value::Object(map) => {
            let mut out = HashMap::new();
            for (k, v) in map {
                out.insert(k.clone(), json_to_value(v));
            }
            Value::Map(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::logic::parse::parse;

    #[test]
    fn mod_as_file_can_be_read() {
        let dir = std::env::temp_dir().join(format!("autoadd_logic_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pack.bin");
        std::fs::write(&path, b"hello").unwrap();
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let fields = HashMap::new();
        let mut host = LogicHost::new(path, work, &fields, None);
        let stmts = parse(
            r#"
var f = mod.as_file()
var data = read_file(f)
return len(data) == 5
"#,
        )
        .expect("parse");
        let result = run(&stmts, &mut host).expect("run");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.success);
    }
}
