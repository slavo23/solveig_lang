#[cfg(test)]
pub mod test_inference {
    use crate::context::Context;
    use crate::inference::{TypeInference, apply_context};
    use core_lang::core_expr::CoreExpr;
    use core_lang::core_type::Ty;

    #[test]
    fn test_id_func() {
        let id_abs = CoreExpr::abs("x".into(), CoreExpr::Var("x".into()));

        let app = CoreExpr::app(id_abs.clone(), id_abs.clone());
        let app_unit = CoreExpr::app(app, CoreExpr::Unit);

        let mut inference = TypeInference::new();
        let ctx = Context::new();

        let (solved_ctx, solved_type) = inference.solve_inference(ctx, &app_unit).unwrap();
        let result_type: Ty = solved_type.clone();

        assert_eq!(apply_context(&solved_ctx, &result_type), Ty::TyUnit)
    }
}
