use core_lang::{core_type::Ty, core_expr::CoreExpr};
use syntax::context::{Context};
use syntax::inference::{TypeInference, apply_context};





fn main() {
    let f_var = CoreExpr::var("f".into());
    let g_var = CoreExpr::var("g".to_string());
    let f1 = CoreExpr::abs("f".to_string(), CoreExpr::app(f_var, g_var));
    let f2 = CoreExpr::abs("g".to_string(), f1.clone());

    let app = CoreExpr::app(f2.clone(), CoreExpr::unit());

    let mut inference = TypeInference::new();
    let ctx = Context::new();

    let (solved_ctx, solved_type) = inference.solve_inference(ctx, &app).unwrap();
    let result_type: Ty = solved_type.clone();

    println!("{:?}", apply_context(&solved_ctx, &result_type).pretty());
}
