use crate::heap::{Heap, HeapObject};
use crate::values::Value;
use solveig_ast::ast::{CtorRuntimeInfo, CtorRuntimeRole};

pub struct DisplayValue<'a> {
    pub root: Value,
    pub heap: &'a Heap,
    pub arities: &'a [CtorRuntimeInfo],
}

impl std::fmt::Display for DisplayValue<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render(self.root))
    }
}

fn tag_name(arities: &[CtorRuntimeInfo], tag: u16) -> String {
    tag_info(arities, tag)
        .map(|info| info.short_name.clone())
        .unwrap_or_else(|| "<?>".to_string())
}

fn tag_info(arities: &[CtorRuntimeInfo], tag: u16) -> Option<&CtorRuntimeInfo> {
    arities.iter().find(|info| info.tag == tag)
}

fn tag_has_role(arities: &[CtorRuntimeInfo], tag: u16, role: CtorRuntimeRole) -> bool {
    tag_info(arities, tag)
        .map(|info| info.role == role)
        .unwrap_or(false)
}

fn tag_has_short_name(arities: &[CtorRuntimeInfo], tag: u16, expected: &str) -> bool {
    tag_info(arities, tag)
        .map(|info| info.short_name == expected)
        .unwrap_or(false)
}

impl DisplayValue<'_> {
    fn render(&self, val: Value) -> String {
        if let Some(s) = self.try_nat(val) {
            return s;
        }
        if let Some(s) = self.try_list(val) {
            return s;
        }
        if let Some(s) = self.try_pair(val) {
            return s;
        }

        match val {
            Value::Unit => "()".into(),
            Value::Int(n) => n.to_string(),
            Value::Float(n) => n.to_string(),
            Value::Char(ch) => format!("'{}'", escape_char_literal(ch)),
            Value::CtorFn { tag, .. } => {
                format!("<ctor {}>", tag_name(self.arities, tag))
            }
            Value::FunctionRef { .. } => "<function>".into(),
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Closure { .. } | HeapObject::PartialApp { .. } => "<closure>".into(),
                HeapObject::PartialCtor {
                    tag, applied_args, ..
                } => {
                    let name = tag_name(self.arities, *tag);
                    let applied_args = self.heap.payloads.as_slice(*applied_args);

                    if applied_args.is_empty() {
                        format!("<partial {}>", name)
                    } else {
                        format!("<partial {} {}>", name, self.render_args(applied_args))
                    }
                }
                HeapObject::Data0 { tag } => tag_name(self.arities, *tag),
                HeapObject::Data1 { tag, field } => {
                    let name = tag_name(self.arities, *tag);
                    format!("{} {}", name, self.render_atom(*field))
                }
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } => {
                    let name = tag_name(self.arities, *tag);
                    format!(
                        "{} {} {}",
                        name,
                        self.render_atom(*field0),
                        self.render_atom(*field1)
                    )
                }
                HeapObject::DataN { tag, fields } => {
                    let name = tag_name(self.arities, *tag);
                    let fields = self.heap.payloads.as_slice(*fields);

                    if fields.is_empty() {
                        name
                    } else {
                        format!("{} {}", name, self.render_args(fields))
                    }
                }
                HeapObject::Str { bytes } => {
                    format!("\"{}\"", escape_string_literal(self.heap.str_value(*bytes)))
                }
            },
        }
    }

    fn render_atom(&self, val: Value) -> String {
        let s = self.render(val);
        if s.contains(' ') {
            format!("({})", s)
        } else {
            s
        }
    }

    fn render_args(&self, vals: &[Value]) -> String {
        vals.iter()
            .map(|&v| self.render_atom(v))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn try_nat(&self, mut val: Value) -> Option<String> {
        let mut n = 0usize;

        loop {
            match val {
                Value::Obj(id) => match self.heap.get(id) {
                    HeapObject::Data0 { tag } => {
                        if tag_has_short_name(self.arities, *tag, "Z") {
                            return Some(n.to_string());
                        }
                        return None;
                    }
                    HeapObject::Data1 { tag, field } => {
                        if tag_has_short_name(self.arities, *tag, "S") {
                            n += 1;
                            val = *field;
                            continue;
                        }
                        return None;
                    }
                    HeapObject::Data2 { .. } | HeapObject::DataN { .. } => return None,
                    _ => return None,
                },
                _ => return None,
            }
        }
    }
    fn try_list(&self, mut val: Value) -> Option<String> {
        let mut items = Vec::new();

        loop {
            match val {
                Value::Obj(id) => match self.heap.get(id) {
                    HeapObject::Data0 { tag } => {
                        if tag_has_role(self.arities, *tag, CtorRuntimeRole::PreludeListNil) {
                            return Some(format!("[{}]", items.join(", ")));
                        }
                        return None;
                    }
                    HeapObject::Data2 {
                        tag,
                        field0,
                        field1,
                    } => {
                        if tag_has_role(self.arities, *tag, CtorRuntimeRole::PreludeListCons) {
                            items.push(self.render(*field0));
                            val = *field1;
                            continue;
                        }
                        return None;
                    }
                    HeapObject::Data1 { .. } | HeapObject::DataN { .. } => return None,
                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn try_pair(&self, val: Value) -> Option<String> {
        match val {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } => {
                    if tag_has_role(self.arities, *tag, CtorRuntimeRole::PreludePair) {
                        Some(format!(
                            "({}, {})",
                            self.render(*field0),
                            self.render(*field1)
                        ))
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }
}

fn escape_char_literal(ch: char) -> String {
    match ch {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '"' => "\\\"".to_string(),
        ch if ch.is_control() => ch.escape_unicode().to_string(),
        ch => ch.to_string(),
    }
}

fn escape_string_literal(value: &str) -> String {
    let mut out = String::new();

    for ch in value.chars() {
        out.push_str(&match ch {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            ch if ch.is_control() => ch.escape_unicode().to_string(),
            ch => ch.to_string(),
        });
    }

    out
}
