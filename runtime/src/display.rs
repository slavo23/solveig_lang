use crate::heap::{Heap, HeapObject};
use crate::values::Value;

pub struct DisplayValue<'a> {
    pub root: Value,
    pub heap: &'a Heap,
    pub arities: &'a [(String, u16, u16)],
}

impl std::fmt::Display for DisplayValue<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render(self.root))
    }
}

fn base_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn tag_name(arities: &[(String, u16, u16)], tag: u16) -> &str {
    arities
        .iter()
        .find(|(_, t, _)| *t == tag)
        .map(|(n, _, _)| base_name(n))
        .unwrap_or("<?>")
}

fn tag_is(arities: &[(String, u16, u16)], tag: u16, expected: &str) -> bool {
    tag_name(arities, tag) == expected
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
            Value::CtorFn { tag, .. } => {
                format!("<ctor {}>", tag_name(self.arities, tag))
            }
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Closure { .. } | HeapObject::PartialApp { .. } => "<closure>".into(),
                HeapObject::PartialCtor { tag, supplied, .. } => {
                    let name = tag_name(self.arities, *tag);
                    let supplied = self.heap.payloads.as_slice(*supplied);

                    if supplied.is_empty() {
                        format!("<partial {}>", name)
                    } else {
                        format!("<partial {} {}>", name, self.render_args(supplied))
                    }
                }
                HeapObject::Data { tag, fields } => {
                    let name = tag_name(self.arities, *tag);
                    let fields = self.heap.payloads.as_slice(*fields);

                    if fields.is_empty() {
                        name.into()
                    } else {
                        format!("{} {}", name, self.render_args(fields))
                    }
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
                    HeapObject::Data { tag, fields } => {
                        let fields = self.heap.payloads.as_slice(*fields);

                        if fields.is_empty() && tag_is(self.arities, *tag, "Z") {
                            return Some(n.to_string());
                        }

                        if fields.len() == 1 && tag_is(self.arities, *tag, "S") {
                            n += 1;
                            val = fields[0];
                            continue;
                        }

                        return None;
                    }
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
                    HeapObject::Data { tag, fields } => {
                        let fields = self.heap.payloads.as_slice(*fields);

                        if fields.is_empty() && tag_is(self.arities, *tag, "Nil") {
                            return Some(format!("[{}]", items.join(", ")));
                        }

                        if fields.len() == 2 && tag_is(self.arities, *tag, "Cons") {
                            items.push(self.render(fields[0]));
                            val = fields[1];
                            continue;
                        }

                        return None;
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn try_pair(&self, val: Value) -> Option<String> {
        match val {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Data { tag, fields } => {
                    let fields = self.heap.payloads.as_slice(*fields);

                    if fields.len() == 2 {
                        let name = tag_name(self.arities, *tag);
                        if name == "Pair" || name == "MkPair" {
                            Some(format!(
                                "({}, {})",
                                self.render(fields[0]),
                                self.render(fields[1])
                            ))
                        } else {
                            None
                        }
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
