use std::error::Error;
use std::io::Read;

use crate::hooks::context::HookContext;
use crate::hooks::{self, HookOutput};

pub fn run(provider: &str, hook_name: &str) -> Result<(), Box<dyn Error>> {
    // Resolve handler (validates provider/hook_name)
    let handler = hooks::dispatch(provider, hook_name)?;

    // Read stdin
    let mut input_str = String::new();
    std::io::stdin().read_to_string(&mut input_str)?;

    let input: serde_json::Value = if input_str.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&input_str)
            .map_err(|e| hooks::HookError::InvalidInput(e.to_string()))?
    };

    let ctx = HookContext::from_input(input);
    let output = handler.execute(&ctx)?;

    match output {
        HookOutput::Silent => {}
        HookOutput::Stdout(text) => println!("{text}"),
        HookOutput::Json(val) => println!("{}", serde_json::to_string(&val).unwrap()),
        HookOutput::Block(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    }

    Ok(())
}
