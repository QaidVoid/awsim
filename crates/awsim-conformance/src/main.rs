use clap::Parser;

mod runner;
mod server;
mod smithy;

#[derive(Parser)]
#[command(
    name = "awsim-conformance",
    about = "Smithy conformance test harness for AWSim"
)]
struct Cli {
    /// Run only specific services (comma-separated, e.g. "dynamodb,s3")
    #[arg(short, long)]
    services: Option<String>,

    /// Path to the Smithy JSON AST models directory
    #[arg(short, long, default_value = "models")]
    models_dir: String,

    /// Show detailed per-operation results
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Suppress tracing output unless RUST_LOG is set.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
        )
        .init();

    // Start AWSim server.
    let endpoint = server::start().await;
    println!("AWSim started at {endpoint}\n");

    // Parse Smithy models.
    let model_dir = std::path::Path::new(&cli.models_dir);
    if !model_dir.exists() {
        eprintln!("Models directory '{}' not found.", cli.models_dir);
        std::process::exit(1);
    }

    let filter: Option<Vec<&str>> = cli
        .services
        .as_ref()
        .map(|s| s.split(',').map(|s| s.trim()).collect());

    let mut all_results: Vec<runner::ServiceResult> = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(model_dir)
        .expect("Failed to read models directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let service_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        // Apply service filter if specified.
        if let Some(ref filter) = filter
            && !filter.iter().any(|f| service_name.contains(f)) {
                continue;
            }

        println!("Testing service: {service_name}");

        let model = smithy::parse_model(&path);

        let result = runner::test_service(&endpoint, &service_name, &model, cli.verbose).await;

        if cli.verbose {
            // Per-operation breakdown.
            for r in &result.results {
                match r {
                    runner::OpResult::Pass(name) => println!("  [PASS] {name}"),
                    runner::OpResult::Fail(name, err) => {
                        println!("  [FAIL] {name}: {}", &err[..err.len().min(200)])
                    }
                    runner::OpResult::NotImplemented(name) => {
                        println!("  [SKIP] {name}: not implemented in AWSim")
                    }
                    runner::OpResult::Skipped(name) => {
                        println!("  [SKIP] {name}: requires prerequisite state")
                    }
                }
            }

            // Operations in the Smithy model but not tested.
            let tested_names: std::collections::HashSet<_> = result
                .results
                .iter()
                .map(|r| r.op_name().to_string())
                .collect();
            let smithy_names = model.operation_names();
            let mut not_tested: Vec<_> = smithy_names
                .iter()
                .filter(|n| !tested_names.contains(*n))
                .collect();
            not_tested.sort();
            if !not_tested.is_empty() {
                println!(
                    "  [INFO] Not covered in this run: {}",
                    not_tested
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        all_results.push(result);
    }

    // ---- Summary ----
    println!();
    println!("{}", "=".repeat(70));
    println!("CONFORMANCE SUMMARY");
    println!("{}", "=".repeat(70));

    let mut total_smithy_ops = 0usize;
    let mut total_tested = 0usize;
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;

    for r in &all_results {
        let status = if r.failed == 0 { "OK " } else { "ERR" };
        let coverage_pct = (r.implemented * 100).checked_div(r.total).unwrap_or(0);
        println!(
            "[{status}] {:<30} {}/{} ops covered ({coverage_pct}%), {} passed, {} failed",
            r.service, r.implemented, r.total, r.passed, r.failed
        );

        total_smithy_ops += r.total;
        total_tested += r.implemented;
        total_passed += r.passed;
        total_failed += r.failed;
    }

    println!("{}", "-".repeat(70));
    let total_pct = (total_tested * 100).checked_div(total_smithy_ops).unwrap_or(0);
    println!("Total: {total_tested}/{total_smithy_ops} operations covered ({total_pct}%)");
    println!("Passed: {total_passed}  Failed: {total_failed}");

    if total_failed > 0 {
        println!(
            "\nFAILED: {} deserialization errors detected.",
            total_failed
        );
        std::process::exit(1);
    } else {
        println!("\nAll tested operations passed shape validation.");
    }
}
