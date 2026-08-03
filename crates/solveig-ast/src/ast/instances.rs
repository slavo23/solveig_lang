//! Instance definitions, instance tables, and overlap checking.
//! The overlap test compares instance heads structurally after separating left and right variables.

use solveig_base::FxHashMap;
use std::ops::Range;

use super::*;

#[derive(Clone, Debug)]
pub struct InstanceMethodDef<A, P = A> {
    pub name: String,
    pub body: TermExprNode<A, P>,
}

impl<A, P> InstanceMethodDef<A, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> InstanceMethodDef<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        InstanceMethodDef {
            name: self.name,
            body: self.body.map_anno(expr_anno, pattern_anno),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstanceDefinition<A, P = A> {
    pub id: InstanceId,
    pub span: Range<usize>,
    pub def_module: ModuleId,
    pub class_module: Option<ModuleId>,
    pub type_module: Option<ModuleId>,
    pub context: Vec<Predicate>,
    pub context_evidence: Vec<EvVarId>,
    pub head: Predicate,
    pub superclass_evidence: Vec<Ev>,
    pub methods: Vec<InstanceMethodDef<A, P>>,
}

impl<A, P> InstanceDefinition<A, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> InstanceDefinition<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        InstanceDefinition {
            id: self.id,
            span: self.span,
            def_module: self.def_module,
            class_module: self.class_module,
            type_module: self.type_module,
            context: self.context,
            context_evidence: self.context_evidence,
            head: self.head,
            superclass_evidence: self.superclass_evidence,
            methods: self
                .methods
                .into_iter()
                .map(|method| method.map_anno(expr_anno, pattern_anno))
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstanceInfo<A, P = A> {
    pub head: Predicate,
    pub context: Vec<Predicate>,
    pub class: ClassId,
    pub def_module: ModuleId,
    pub methods: Vec<InstanceMethodDef<A, P>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceOverlap {
    pub first: InstanceId,
    pub second: InstanceId,
    pub class: ClassId,
    pub instance_args: Vec<Ty>,
}

#[derive(Clone, Debug, Default)]
pub struct InstanceTable<A, P = A> {
    entries: Vec<(InstanceId, InstanceInfo<A, P>)>,
}

impl<A, P> InstanceTable<A, P> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn from_definitions(definitions: Vec<InstanceDefinition<A, P>>) -> Self {
        let mut table = Self::new();

        for definition in definitions {
            table.insert_definition(definition);
        }

        table
    }

    pub fn insert(&mut self, info: InstanceInfo<A, P>) -> InstanceId {
        let id = InstanceId(self.entries.len() as u32);
        self.entries.push((id, info));
        id
    }

    pub fn insert_definition(&mut self, definition: InstanceDefinition<A, P>) -> InstanceId {
        let InstanceDefinition {
            id,
            span: _,
            def_module,
            class_module: _,
            type_module: _,
            context,
            context_evidence: _,
            head,
            superclass_evidence: _,
            methods,
        } = definition;
        let class = head.class_id;

        self.entries.push((
            id,
            InstanceInfo {
                head,
                context,
                class,
                def_module,
                methods,
            },
        ));
        id
    }

    pub fn iter(&self) -> impl Iterator<Item = (InstanceId, &InstanceInfo<A, P>)> {
        self.entries.iter().map(|(id, info)| (*id, info))
    }

    pub fn get(&self, id: InstanceId) -> Option<&InstanceInfo<A, P>> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, info)| info)
    }

    pub fn overlaps(&self) -> Vec<InstanceOverlap> {
        overlaps_from_heads(
            self.entries
                .iter()
                .map(|(id, info)| (*id, info.class, &info.head)),
        )
    }
}

/// Returns overlapping instance heads from borrowed instance definitions.
pub fn instance_overlaps<A, P>(definitions: &[InstanceDefinition<A, P>]) -> Vec<InstanceOverlap> {
    overlaps_from_heads(
        definitions
            .iter()
            .map(|definition| (definition.id, definition.head.class_id, &definition.head)),
    )
}

fn overlaps_from_heads<'a>(
    heads: impl Iterator<Item = (InstanceId, ClassId, &'a Predicate)>,
) -> Vec<InstanceOverlap> {
    let heads = heads.collect::<Vec<_>>();
    let mut overlaps = Vec::new();

    for (left_index, (left_id, left_class, left_head)) in heads.iter().enumerate() {
        for (right_id, right_class, right_head) in heads.iter().skip(left_index + 1) {
            if left_class != right_class || left_head.args.len() != right_head.args.len() {
                continue;
            }

            if instance_heads_unify(left_head, right_head) {
                overlaps.push(InstanceOverlap {
                    first: *left_id,
                    second: *right_id,
                    class: *left_class,
                    instance_args: right_head.args.clone(),
                });
            }
        }
    }

    overlaps
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum InstanceUnifySide {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InstanceUnifyTy {
    Unit,
    Var(InstanceUnifySide, TyVarId),
    Evar(EvarId),
    Con(TyConId),
    App(Box<InstanceUnifyTy>, Box<InstanceUnifyTy>),
    Arr(Box<InstanceUnifyTy>, Box<InstanceUnifyTy>),
}

type InstanceUnifySubst = FxHashMap<(InstanceUnifySide, TyVarId), InstanceUnifyTy>;

fn instance_heads_unify(left: &Predicate, right: &Predicate) -> bool {
    if left.class_id != right.class_id || left.args.len() != right.args.len() {
        return false;
    }

    let mut subst = FxHashMap::default();

    left.args
        .iter()
        .zip(right.args.iter())
        .all(|(left, right)| {
            let left_ty = InstanceUnifyTy::from_type(InstanceUnifySide::Left, left);
            let right_ty = InstanceUnifyTy::from_type(InstanceUnifySide::Right, right);
            unify_instance_types(&left_ty, &right_ty, &mut subst)
        })
}

impl InstanceUnifyTy {
    fn from_type(side: InstanceUnifySide, ty: &Ty) -> Self {
        match ty {
            Ty::TyUnit => Self::Unit,
            Ty::TyVar(id) => Self::Var(side, *id),
            Ty::TyExVar(id) => Self::Evar(*id),
            Ty::TyCon(id) => Self::Con(*id),
            Ty::TyApp(func, arg) => Self::App(
                Box::new(Self::from_type(side, func)),
                Box::new(Self::from_type(side, arg)),
            ),
            Ty::TyArr(lhs, rhs) => Self::Arr(
                Box::new(Self::from_type(side, lhs)),
                Box::new(Self::from_type(side, rhs)),
            ),
            Ty::TyForAll(_, _, body) => Self::from_type(side, body),
        }
    }
}

fn unify_instance_types(
    left: &InstanceUnifyTy,
    right: &InstanceUnifyTy,
    subst: &mut InstanceUnifySubst,
) -> bool {
    let left = apply_instance_unify_subst(left, subst);
    let right = apply_instance_unify_subst(right, subst);

    match (left, right) {
        (InstanceUnifyTy::Var(side, id), ty) | (ty, InstanceUnifyTy::Var(side, id)) => {
            bind_instance_unify_var(side, id, &ty, subst)
        }
        (InstanceUnifyTy::Unit, InstanceUnifyTy::Unit) => true,
        (InstanceUnifyTy::Evar(a), InstanceUnifyTy::Evar(b)) => a == b,
        (InstanceUnifyTy::Con(a), InstanceUnifyTy::Con(b)) => a == b,
        (
            InstanceUnifyTy::App(left_func, left_arg),
            InstanceUnifyTy::App(right_func, right_arg),
        ) => {
            unify_instance_types(&left_func, &right_func, subst)
                && unify_instance_types(&left_arg, &right_arg, subst)
        }
        (InstanceUnifyTy::Arr(left_arg, left_ret), InstanceUnifyTy::Arr(right_arg, right_ret)) => {
            unify_instance_types(&left_arg, &right_arg, subst)
                && unify_instance_types(&left_ret, &right_ret, subst)
        }
        _ => false,
    }
}

fn bind_instance_unify_var(
    side: InstanceUnifySide,
    id: TyVarId,
    ty: &InstanceUnifyTy,
    subst: &mut InstanceUnifySubst,
) -> bool {
    let ty = apply_instance_unify_subst(ty, subst);

    if ty == InstanceUnifyTy::Var(side, id) {
        return true;
    }

    if instance_unify_occurs(side, id, &ty, subst) {
        return false;
    }

    subst.insert((side, id), ty);
    true
}

fn apply_instance_unify_subst(ty: &InstanceUnifyTy, subst: &InstanceUnifySubst) -> InstanceUnifyTy {
    match ty {
        InstanceUnifyTy::Var(side, id) => subst.get(&(*side, *id)).map_or_else(
            || ty.clone(),
            |bound| apply_instance_unify_subst(bound, subst),
        ),
        InstanceUnifyTy::App(func, arg) => InstanceUnifyTy::App(
            Box::new(apply_instance_unify_subst(func, subst)),
            Box::new(apply_instance_unify_subst(arg, subst)),
        ),
        InstanceUnifyTy::Arr(lhs, rhs) => InstanceUnifyTy::Arr(
            Box::new(apply_instance_unify_subst(lhs, subst)),
            Box::new(apply_instance_unify_subst(rhs, subst)),
        ),
        InstanceUnifyTy::Unit | InstanceUnifyTy::Evar(_) | InstanceUnifyTy::Con(_) => ty.clone(),
    }
}

fn instance_unify_occurs(
    side: InstanceUnifySide,
    id: TyVarId,
    ty: &InstanceUnifyTy,
    subst: &InstanceUnifySubst,
) -> bool {
    match apply_instance_unify_subst(ty, subst) {
        InstanceUnifyTy::Var(var_side, var_id) => var_side == side && var_id == id,
        InstanceUnifyTy::App(func, arg) | InstanceUnifyTy::Arr(func, arg) => {
            instance_unify_occurs(side, id, &func, subst)
                || instance_unify_occurs(side, id, &arg, subst)
        }
        InstanceUnifyTy::Unit | InstanceUnifyTy::Evar(_) | InstanceUnifyTy::Con(_) => false,
    }
}
