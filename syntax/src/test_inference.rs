// #[cfg(test)]
// pub mod test_inference {
//     use crate::context::Context;
//     use crate::inference::{TypeInference, apply_context};
//     use core::core_expr::CoreExpr;
//     use core::core_type::Ty;

//     #[test]
//     fn test_id_func() {
//         let id_abs = CoreExpr::abs("x".into(), CoreExpr::Var("x".into()));
//         let anno_id = CoreExpr::anno(
//             id_abs,
//             Ty::make_forall(
//                 "t".into(),
//                 Ty::make_arr(Ty::make_var("t".into()), Ty::make_var("t".into())),
//             ),
//         );

//         let app_unit = CoreExpr::app(anno_id, CoreExpr::Unit);

//         let mut inference = TypeInference::new();
//         let ctx = Context::new();

//         let (solved_ctx, solved_type) = inference.solve_inference(ctx, &app_unit).unwrap();
//         let result_type: Ty = solved_type.clone();

//         assert_eq!(apply_context(&solved_ctx, &result_type), Ty::TyUnit)
//     }

//     #[test]
//     fn test_flip() {
//         // flip : \f. \x. \y. (f y) x
//         let f_var = CoreExpr::var("f".into());
//         let x_var = CoreExpr::var("x".into());
//         let y_var = CoreExpr::var("y".into());

//         let flip = CoreExpr::abs(
//             "f".into(),
//             CoreExpr::abs(
//                 "x".into(),
//                 CoreExpr::abs(
//                     "y".into(),
//                     CoreExpr::app(CoreExpr::app(f_var, y_var), x_var),
//                 ),
//             ),
//         );

//         let mut inference = TypeInference::new();
//         let ctx = Context::new();

//         let (solved_ctx, solved_type) = inference.solve_inference(ctx, &flip).unwrap();
//         let result_type: Ty = solved_type.clone();

//         println!("{}", apply_context(&solved_ctx, &result_type).pretty());
//     }

//     #[test]
//     fn test_let() {
//         let id_abs = CoreExpr::abs("x".into(), CoreExpr::var("x".into()));
//         let let_expr = CoreExpr::let_expr("id".into(), id_abs, CoreExpr::var("id".into()));

//         let mut inference = TypeInference::new();
//         let ctx = Context::new();

//         let (solved_ctx, solved_type) = inference.solve_inference(ctx, &let_expr).unwrap();
//         let result_type: Ty = solved_type.clone();

//         println!("{}", apply_context(&solved_ctx, &result_type).pretty());
//     }

//     #[test]
//     fn test_fix() {
//         let g = CoreExpr::abs(
//             "x".into(),
//             CoreExpr::abs("y".into(), CoreExpr::var("y".into())),
//         );

//         let fix = CoreExpr::fix(g);
//         let app = CoreExpr::app(fix, CoreExpr::Unit);

//         let mut inference2 = TypeInference::new();
//         let ctx2 = Context::new();

//         let (solved_ctx, solved_ty) = inference2.solve_inference(ctx2, &app).unwrap();
//         let solved = apply_context(&solved_ctx, &solved_ty);

//         assert!(
//             matches!(solved, Ty::TyUnit),
//             "expected Unit, got {}",
//             solved.pretty()
//         );
//     }
// }
