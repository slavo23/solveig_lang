use crate::heap::{Heap, HeapObject};
use crate::values::Value;
use solveig_ast::ast::CtorRuntimeInfo;

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
    arities
        .iter()
        .find(|info| info.tag == tag)
        .map(|info| info.short_name.clone())
        .unwrap_or_else(|| "<?>".to_string())
}

impl DisplayValue<'_> {
    fn render(&self, val: Value) -> String {
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
                HeapObject::IoAction(_) => "<io>".into(),
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
