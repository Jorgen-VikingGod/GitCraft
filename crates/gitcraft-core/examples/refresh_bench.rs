//! Times one full refresh of a repository the way the UI does it: status, refs and log together.
//! `cargo run --release -p gitcraft-core --example refresh_bench -- <repo> [rounds]`

use std::time::Instant;

use gitcraft_core::model::{LogFilter, LogOptions};
use gitcraft_core::{queries, runner};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let repo = args.next().unwrap_or_else(|| ".".to_owned());
    let rounds: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(5);
    let options = LogOptions {
        stashes: true,
        ..LogOptions::default()
    };
    println!("{:?}", runner::git_version_text().await);
    let started = Instant::now();
    for _ in 0..20 {
        runner::git(&repo, &["rev-parse", "--show-toplevel"])
            .await
            .expect("rev-parse");
    }
    println!(
        "rev-parse in sequence: {:.1} ms each",
        started.elapsed().as_secs_f64() * 1000.0 / 20.0
    );
    let filter = LogFilter::default();
    for round in 0..rounds {
        let started = Instant::now();
        let ((status, refs, log), spawns) = runner::count_spawns(async {
            tokio::join!(
                queries::status(&repo),
                queries::refs(&repo),
                queries::log_filtered(&repo, 2000, true, None, &filter, &options),
            )
        })
        .await;
        status.expect("status");
        refs.expect("refs");
        let commits = log.expect("log").commits.len();
        println!(
            "round {round}: {spawns} git processes, {commits} commits, {:.0} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
}
