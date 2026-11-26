//! `ponko-mcp`: MCP server for Ponko games, over stdio.
//!
//!   ponko-mcp              all tools
//!   ponko-mcp --read-only  only tools that don't change games

#[cfg(not(target_arch = "wasm32"))]
mod web_bridge;

fn main() {
    let mut config = ponko_mcp::Config::from_env();
    let mut bridge = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--web-bridge" => bridge = true,
            "--read-only" => config.read_only = true,
            "--help" | "-h" => {
                println!(
                    "ponko-mcp: MCP server (stdio) for Ponko games\n\n  --read-only  hide tools that change games\n  --web-bridge  register a local browser tab for control tools"
                );
                return;
            }
            other => {
                eprintln!("ponko-mcp: unknown argument `{other}` (try --help)");
                std::process::exit(2);
            }
        }
    }
    if bridge && config.read_only {
        eprintln!("ponko-mcp: --read-only applies to an MCP server, not --web-bridge");
        std::process::exit(2);
    }
    let result = if bridge {
        #[cfg(not(target_arch = "wasm32"))]
        {
            web_bridge::run(&config.instances_dir)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err("the browser control bridge must run on the host".into())
        }
    } else {
        ponko_mcp::serve_stdio(config).map_err(Into::into)
    };
    if let Err(e) = result {
        eprintln!("ponko-mcp: {e}");
        std::process::exit(1);
    }
}
