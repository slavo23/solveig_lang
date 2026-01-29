#[cfg(test)]
pub mod test_inference {
    use crate::context::Context;
    use crate::inference::{TypeInference, apply_context};
    use core_lang::core_expr::CoreExpr;
    use core_lang::core_type::Ty;

    #[test]
    fn test_id_func() {
        let id_abs = CoreExpr::abs("x".into(), CoreExpr::Var("x".into()));
        let anno_id = CoreExpr::anno(
            id_abs,
            Ty::make_forall(
                "t".into(),
                Ty::make_arr(Ty::make_var("t".into()), Ty::make_var("t".into())),
            ),
        );

        let app_unit = CoreExpr::app(anno_id, CoreExpr::Unit);

        let mut inference = TypeInference::new();
        let ctx = Context::new();

        let (solved_ctx, solved_type) = inference.solve_inference(ctx, &app_unit).unwrap();
        let result_type: Ty = solved_type.clone();

        assert_eq!(apply_context(&solved_ctx, &result_type), Ty::TyUnit)
    }

    #[test]
    fn test_flip() {
        // flip : \f. \x. \y. (f y) x
        let f_var = CoreExpr::var("f".into());
        let x_var = CoreExpr::var("x".into());
        let y_var = CoreExpr::var("y".into());

        let flip = CoreExpr::abs(
            "f".into(),
            CoreExpr::abs(
                "x".into(),
                CoreExpr::abs(
                    "y".into(),
                    CoreExpr::app(CoreExpr::app(f_var, y_var), x_var),
                ),
            ),
        );

        let mut inference = TypeInference::new();
        let ctx = Context::new();

        let (solved_ctx, solved_type) = inference.solve_inference(ctx, &flip).unwrap();
        let result_type: Ty = solved_type.clone();

        println!("{}", apply_context(&solved_ctx, &result_type).pretty());
    }
}
