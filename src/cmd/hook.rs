use std::error::Error;

pub fn run(provider: &str, hook_name: &str) -> Result<(), Box<dyn Error>> {
    // Phase 3: full hook replacement — parse stdin JSON, emit metrics, parse session
    // For now, just validate args
    eprintln!("hook command not yet implemented: {provider}/{hook_name}");
    Ok(())
}
